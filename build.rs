use clap_complete::{generate_to, Shell};
use std::env;

include!("src/cli.rs");

const GENERATED_COMPLETIONS: &[Shell] = &[Shell::Bash, Shell::Zsh, Shell::Fish];

fn generate_completions() {
    let mut app = build_cli();
    for shell in GENERATED_COMPLETIONS {
        generate_to(*shell, &mut app, "ciel", "completions")
            .expect("Failed to generate shell completions");
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=CIEL_GEN_COMPLETIONS");

    // generate completions on demand
    if env::var("CIEL_GEN_COMPLETIONS").is_ok() {
        generate_completions();
    }
}
