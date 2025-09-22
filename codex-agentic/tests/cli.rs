#[test]
fn help_lists_subcommands_and_usage() {
    let mut cmd = assert_cmd::Command::cargo_bin("codex-agentic").expect("bin");
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Usage: codex-agentic"))
        .stdout(predicates::str::contains("Commands:"))
        .stdout(predicates::str::contains("acp"))
        .stdout(predicates::str::contains("cli"));
}

#[test]
fn help_recipes_prints_examples() {
    let mut cmd = assert_cmd::Command::cargo_bin("codex-agentic").expect("bin");
    cmd.arg("help-recipes");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("YOLO mode"))
        .stdout(predicates::str::contains("codex-agentic acp --oss"));
}
