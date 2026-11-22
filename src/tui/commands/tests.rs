use super::*;

#[test]
fn aliases_resolve_to_the_same_command() {
    assert_eq!(find("clear").unwrap().name, "new");
    assert_eq!(find("exit").unwrap().name, "quit");
    assert_eq!(find("ctx").unwrap().name, "context");
    assert_eq!(find("thinking").unwrap().name, "effort");
    assert_eq!(find("/model").unwrap().name, "model");
}

#[test]
fn prefix_queries_rank_the_intended_command_first() {
    assert_eq!(matches("mod")[0].name, "model");
    assert_eq!(matches("eff")[0].name, "effort");
    assert_eq!(matches("perm")[0].name, "permissions");
    assert_eq!(matches("resu")[0].name, "resume");
    assert_eq!(matches("doc")[0].name, "doctor");
    assert_eq!(matches("chan")[0].name, "changelog");
}

#[test]
fn every_command_appears_in_help() {
    let help = help_text();
    for command in COMMANDS {
        assert!(help.contains(&format!("/{}", command.name)), "missing {}", command.name);
    }
}

#[test]
fn unknown_query_returns_nothing() {
    assert!(matches("zzzzzz").is_empty());
}
