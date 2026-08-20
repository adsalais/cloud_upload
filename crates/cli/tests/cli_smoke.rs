// Vérifie que le binaire s'exécute et pilote un cycle complet.
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_intake"))
}

#[test]
fn cli_create_and_teardown() {
    let id = "cli-smoke-0001";
    // teardown préventif (ignore le code retour)
    let _ = bin().args(["teardown-case", id, "--yes"]).status();

    let create = bin().args(["create-case", id]).output().unwrap();
    assert!(create.status.success(), "create-case a échoué : {create:?}");
    let stdout = String::from_utf8_lossy(&create.stdout);
    assert!(stdout.contains("site_url"), "sortie inattendue : {stdout}");

    let teardown = bin().args(["teardown-case", id, "--yes"]).output().unwrap();
    assert!(teardown.status.success(), "teardown-case a échoué : {teardown:?}");
}
