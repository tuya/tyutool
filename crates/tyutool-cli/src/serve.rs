// Thin re-export: the WebSocket dev-serve implementation now lives in the
// shared `tyutool-serve` crate so other hosts can reuse it unchanged.
pub use tyutool_serve::run_serve;
