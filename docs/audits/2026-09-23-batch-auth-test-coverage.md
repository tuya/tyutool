# Batch authorization ledger safety audit

## Scope and design

Batch authorization runs in `crates/tyutool-core/src/authorize.rs` and persists
row updates through `crates/tyutool-core/src/batch_slot.rs` into the Excel
allocator in `crates/tyutool-core/src/batch_auth.rs`. The allocator recognizes
`AUTHWRITTEN` as a non-available, in-progress status after reload. The update
callback now returns an error so failure to save the reservation aborts before
the serial `auth` command.

Before sending the command, the flow saves `STATUS=AUTHWRITTEN` and
`STEP=auth_write_started`. This is a conservative reservation: it means the
command may or may not have reached the device, not that a write was confirmed.
After a successful command response, the step becomes `auth_written`. A cancel
that arrives during intent persistence is checked again before serial send; the
row remains reserved and the result is `CancelledAfterWrite`. Ambiguous
send/acknowledgment errors also retain the reservation. The new and old firmware
branches share this ordering.

The slot adapter remembers only a successfully persisted write-intent callback.
An `auth_write` error whose diagnostic read cannot confirm the requested values
emits `write_uncertain`; any later error after intent persistence is classified
the same way. Failure to persist the intent leaves that marker unset and remains
a normal retryable failure because no serial auth command was sent. In the UI,
uncertain slots keep a quarantine badge, are excluded from Retry Failed, per-port
Retry, and Start All, and retain the flag across read-only probes. The operator
must explicitly remove or reassign a quarantined port only after confirming a
different device is connected. Automated tests verify workbook save/reload
behavior but do not test power-loss durability or filesystem flush guarantees.

## Automated coverage

Feature-gated tests execute the batch authorization transaction through its
private test opener seam with `MockAuthIo`, a test-supplied Excel callback, and a
temporary workbook containing synthetic credentials. They reload the workbook
through `ExcelRowAllocator` and check the bound MAC, status, remaining count,
in-progress count, result, and whether the `auth` command reached the mock.

Coverage includes cancellation before intent (no command, `MACREAD`), cancellation
after command attempt on both firmware paths (`AUTHWRITTEN`), intent-save failure
(no command), cancellation during intent save (reservation retained, no command),
ambiguous send failure with and without concurrent cancellation on both firmware
paths, and frontend handling of the write-uncertain event plus retry/start
exclusion.
Command: `cargo test --manifest-path 'D:\LENOVO\Documents\github\tyutool\Cargo.toml' -p tyutool-core --features excel batch_auth_`.
Result: **11 passed, 0 failed, 0 ignored**. `cargo fmt --manifest-path 'D:\LENOVO\Documents\github\tyutool\Cargo.toml' --all --check` and
`cargo clippy --manifest-path 'D:\LENOVO\Documents\github\tyutool\Cargo.toml' -p tyutool-core --all-targets --features excel -- -D warnings` also passed.
The store and archive Vitest files passed **150 tests** (2 files). Coverage
includes uncertain-event state, bulk/single retry rejection, Start All exclusion
after cancellation or write uncertainty (including after a read-only probe),
strict assertions for the complete authorization-start payload, behavior when
all ports are occupied, partial Read All with release of the acquired port, and
archive CSV output.

`MACREAD` is also MAC-bound and in-progress; the earlier test gap did not establish
immediate reallocation. It established that a potentially sent command was not
recorded as `AUTHWRITTEN`.

The tests do not exercise Tauri event rendering, actual Excel file-system faults,
or real serial hardware. Non-cancel ambiguous send-error cases cover both
firmware paths; the flow retries and performs its existing diagnostic auth-read,
and the row remains reserved throughout. Hardware verification is still required before
production use of the changed operator workflow.

## Manual acceptance checklist

Hardware execution is **pending**. Use only a physically isolated, expendable
test board, a copied test workbook, and synthetic format-valid credentials. Never
use production credentials or a production workbook. OTP writes are irreversible;
use a dedicated OTP-capable test board or supported KV storage.

1. Record the board/chip, firmware, storage mode, copied workbook, and starting
   row. Verify the board is disconnected from any production station.
2. Cancel before authorization begins. Use serial capture to prove no `auth`
   command was sent. Reopen the workbook and check the row remains MAC-bound with
   `STATUS=MACREAD`.
3. On a separate fresh test row/device, cancel during the authorization window.
   If the operator cannot prove whether cancel occurred before or after `auth`,
   record the run as **inconclusive**, not pass. Confirm the UI labels the device
   authorization state uncertain, and the workbook keeps it at `AUTHWRITTEN` with
   `STEP=auth_write_started` or `auth_written`.
4. Never reuse a row or device that may have received `auth`. Physically isolate
   it and verify device state individually before disposition. Reopen the copied
   workbook and check MAC, status, step, remaining, and in-progress totals.
5. Preserve only synthetic test files; redact serial traces before sharing.

No physical board or manual authorization write was used for this audit.
