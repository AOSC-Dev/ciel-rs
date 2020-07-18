//! This module contains systemd machined related APIs

use crate::common::CIEL_INST_DIR;
use crate::dbus_machine1;
use crate::dbus_machine1_machine;

struct CielInstance {
    name: String,
    // namespace name (in the form of `$name-$id`)
    ns_name: String,
    mounted: bool,
    running: bool,
    booted: bool,
}
