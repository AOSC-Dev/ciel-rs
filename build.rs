use clap_complete::{generate_to, Shell};
use dbus_codegen::GenOpts;
use std::env;
use std::{fs, path::Path};

include!("src/cli.rs");

const MACHINE1_DEF: &str = "dbus-xml/org.freedesktop.machine1.xml";
const MACHINE1_MACHINE_DEF: &str = "dbus-xml/org.freedesktop.machine1-machine.xml";
const GENERATED_COMPLETIONS: &[Shell] = &[Shell::Bash, Shell::Zsh, Shell::Fish];

fn generate_completions() {
    let mut app = build_cli();
    for shell in GENERATED_COMPLETIONS {
        generate_to(*shell, &mut app, "ciel", "completions")
            .expect("Failed to generate shell completions");
    }
}

fn generate_dbus_binding(xmldata: String, name: &str) {
    let options = GenOpts {
        methodtype: None,
        ..Default::default()
    };
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join(name),
        dbus_codegen::generate(&xmldata, &options)
            .unwrap_or_else(|_| panic!("Failed to generate dbus bindings for {}", name))
            .as_bytes(),
    )
    .unwrap();
}

fn main() {
    let machine1 = fs::read_to_string(MACHINE1_DEF).expect("");
    let machine1_machine = fs::read_to_string(MACHINE1_MACHINE_DEF).expect("");
    generate_dbus_binding(machine1, "dbus_machine1.rs");
    generate_dbus_binding(machine1_machine, "dbus_machine1_machine.rs");
    println!("cargo:rerun-if-changed=dbus-xml/org.freedesktop.machine1.xml");
    println!("cargo:rerun-if-changed=dbus-xml/org.freedesktop.machine1-machine.xml");
    println!("cargo:rerun-if-env-changed=CIEL_GEN_COMPLETIONS");

    // generate completions on demand
    if env::var("CIEL_GEN_COMPLETIONS").is_ok() {
        generate_completions();
    }
}
