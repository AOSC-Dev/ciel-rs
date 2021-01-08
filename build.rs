use dbus_codegen::GenOpts;
use std::env;
use std::{fs, path::Path};

const MACHINE1_DEF: &'static str = "dbus-xml/org.freedesktop.machine1.xml";
const MACHINE1_MACHINE_DEF: &'static str = "dbus-xml/org.freedesktop.machine1-machine.xml";

fn main() {
    let machine1 = fs::read_to_string(MACHINE1_DEF).expect("");
    let machine1_machine = fs::read_to_string(MACHINE1_MACHINE_DEF).expect("");
    let mut options = GenOpts::default();
    options.methodtype = None;
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("dbus_machine1.rs"),
        dbus_codegen::generate(&machine1, &options)
            .expect("Failed to generate dbus bindings for machine1"),
    )
    .unwrap();
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("dbus_machine1_machine.rs"),
        dbus_codegen::generate(&machine1_machine, &options)
            .expect("Failed to generate dbus bindings for machine1_machine"),
    )
    .unwrap();
    println!("cargo:rerun-if-changed=dbus-xml/org.freedesktop.machine1.xml");
    println!("cargo:rerun-if-changed=dbus-xml/org.freedesktop.machine1-machine.xml");
}
