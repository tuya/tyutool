//! Known Tuya / dev-board USB dual-serial role hints: `(VID, PID, interface)` → role string for UI.
//!
//! Populate [`USB_PORT_ROLE_RULES`] from hardware docs and `tmp/usb-port-survey.md` measurements.
//! Wrong rules mislabel ports — prefer empty table until verified.

/// Stable machine keys consumed by the Vue i18n tree `flash.portRoles.*`.
pub type PortRoleStr = &'static str;

// Used in [`USB_PORT_ROLE_RULES`] and tests once rules are populated.
#[allow(dead_code)]
pub const ROLE_FLASH_AUTH: PortRoleStr = "flash_auth";
#[allow(dead_code)]
pub const ROLE_LOG: PortRoleStr = "log";

struct UsbPortRoleRule {
    vid: u16,
    pid: u16,
    /// `Some(n)` = match only this USB interface number; `None` = match any interface (single-COM devices).
    usb_interface: Option<u8>,
    role: PortRoleStr,
}

/// Ordered rules: first match wins.
static USB_PORT_ROLE_RULES: &[UsbPortRoleRule] = &[
    // Example (disabled until verified — duplicate VID/PID chips exist):
    // UsbPortRoleRule {
    //     vid: 0x1a86,
    //     pid: 0x7523,
    //     usb_interface: Some(0),
    //     role: ROLE_FLASH_AUTH,
    // },
];

fn infer_from_rules(
    rules: &[UsbPortRoleRule],
    vid: u16,
    pid: u16,
    usb_interface: Option<u8>,
) -> Option<PortRoleStr> {
    for r in rules {
        if r.vid != vid || r.pid != pid {
            continue;
        }
        match r.usb_interface {
            Some(expected) => {
                if usb_interface == Some(expected) {
                    return Some(r.role);
                }
            }
            None => return Some(r.role),
        }
    }
    None
}

/// Returns `flash_auth`, `log`, or `None` when unknown / non-USB.
pub fn infer_usb_port_role(vid: u16, pid: u16, usb_interface: Option<u8>) -> Option<PortRoleStr> {
    infer_from_rules(USB_PORT_ROLE_RULES, vid, pid, usb_interface)
}

#[cfg(test)]
mod tests {
    use super::{
        infer_from_rules, infer_usb_port_role, UsbPortRoleRule, ROLE_FLASH_AUTH, ROLE_LOG,
    };

    #[test]
    fn unknown_vid_pid_returns_none() {
        assert_eq!(infer_usb_port_role(0xffff, 0xffff, Some(0)), None);
    }

    #[test]
    fn empty_rules_table_yields_none_for_common_ch340() {
        assert_eq!(infer_usb_port_role(0x1a86, 0x7523, Some(0)), None);
    }

    #[test]
    fn rule_match_first_wins() {
        let rules: &[UsbPortRoleRule] = &[
            UsbPortRoleRule {
                vid: 0xaaaa,
                pid: 0xbbbb,
                usb_interface: Some(1),
                role: ROLE_LOG,
            },
            UsbPortRoleRule {
                vid: 0xaaaa,
                pid: 0xbbbb,
                usb_interface: Some(0),
                role: ROLE_FLASH_AUTH,
            },
        ];
        assert_eq!(
            infer_from_rules(rules, 0xaaaa, 0xbbbb, Some(0)),
            Some(ROLE_FLASH_AUTH)
        );
        assert_eq!(
            infer_from_rules(rules, 0xaaaa, 0xbbbb, Some(1)),
            Some(ROLE_LOG)
        );
        assert_eq!(infer_from_rules(rules, 0xaaaa, 0xbbbb, Some(2)), None);
    }
}
