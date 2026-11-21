//! Command risk classification for the `bash` tool.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashRisk {
    /// Read-only command that is safe to auto-approve.
    Safe,
    /// Unknown side effects; ask the user unless a rule allows it.
    Risky,
    /// Destructive command that is denied in every mode.
    Dangerous,
}

const DANGEROUS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $home",
    "rm -fr /",
    "mkfs.",
    "dd if=",
    ":(){",
    "chmod -r 777 /",
    "chown -r ",
    "> /dev/sd",
    "shutdown",
    "reboot",
    "halt",
];

pub fn classify(command: &str) -> BashRisk {
    let lowered = command.to_ascii_lowercase().replace("  ", " ");

    for pattern in DANGEROUS {
        if lowered.contains(pattern) {
            // `dd if=` is only dangerous when it writes to a device.
            if *pattern == "dd if=" && !lowered.contains("of=/dev/") {
                continue;
            }
            return BashRisk::Dangerous;
        }
    }

    // Any redirection may write to disk.
    if command.contains('>') || command.contains("<<") {
        return BashRisk::Risky;
    }

    let segments = split_segments(command);
    if segments.is_empty() {
        return BashRisk::Risky;
    }

    let mut all_safe = true;
    for segment in segments {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some(program) = segment.split_whitespace().next() else {
            continue;
        };
        let program = program.rsplit('/').next().unwrap_or(program);
        let subcommand_ok = match program {
            "git" => is_safe_git(segment),
            "cargo" => is_safe_cargo(segment),
            _ => true,
        };
        if !is_safe_program(program) || !subcommand_ok {
            all_safe = false;
            break;
        }
    }

    if all_safe {
        BashRisk::Safe
    } else {
        BashRisk::Risky
    }
}

fn split_segments(command: &str) -> Vec<&str> {
    let mut segments = vec![command];
    for separator in ["&&", "||", ";", "|", "\n"] {
        segments = segments
            .into_iter()
            .flat_map(|part| part.split(separator))
            .collect();
    }
    segments
}

fn is_safe_program(program: &str) -> bool {
    matches!(
        program,
        "ls" | "cat"
            | "head"
            | "tail"
            | "wc"
            | "pwd"
            | "echo"
            | "printf"
            | "which"
            | "whoami"
            | "date"
            | "uname"
            | "find"
            | "rg"
            | "grep"
            | "fd"
            | "sort"
            | "uniq"
            | "cut"
            | "tr"
            | "diff"
            | "stat"
            | "file"
            | "du"
            | "df"
            | "tree"
            | "bat"
            | "jq"
            | "git"
            | "cargo"
    )
}

fn is_safe_git(segment: &str) -> bool {
    let mut words = segment.split_whitespace();
    let _ = words.next();
    let Some(subcommand) = words.next() else {
        return false;
    };
    matches!(
        subcommand,
        "status"
            | "diff"
            | "log"
            | "show"
            | "branch"
            | "remote"
            | "rev-parse"
            | "describe"
            | "blame"
            | "ls-files"
            | "ls-tree"
            | "shortlog"
            | "tag"
    ) && !segment.contains(" -d ")
        && !segment.contains(" -D ")
        && !segment.contains(" --delete")
}

fn is_safe_cargo(segment: &str) -> bool {
    let mut words = segment.split_whitespace();
    let _ = words.next();
    let Some(subcommand) = words.next() else {
        return false;
    };
    matches!(
        subcommand,
        "check" | "build" | "clippy" | "fmt" | "metadata" | "tree" | "--version" | "locate-project"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_commands_are_safe() {
        assert_eq!(classify("ls -la"), BashRisk::Safe);
        assert_eq!(classify("git status"), BashRisk::Safe);
        assert_eq!(classify("grep -r foo . | head -20"), BashRisk::Safe);
        assert_eq!(classify("cargo check && cargo build"), BashRisk::Safe);
    }

    #[test]
    fn unknown_commands_are_risky() {
        assert_eq!(classify("npm install"), BashRisk::Risky);
        assert_eq!(classify("python script.py"), BashRisk::Risky);
        assert_eq!(classify("echo hi > out.txt"), BashRisk::Risky);
        assert_eq!(classify("git push"), BashRisk::Risky);
    }

    #[test]
    fn destructive_commands_are_dangerous() {
        assert_eq!(classify("rm -rf /"), BashRisk::Dangerous);
        assert_eq!(classify("rm -rf ~"), BashRisk::Dangerous);
        assert_eq!(classify("mkfs.ext4 /dev/sda1"), BashRisk::Dangerous);
        assert_eq!(classify("dd if=/dev/zero of=/dev/sda"), BashRisk::Dangerous);
    }

    #[test]
    fn safe_dd_without_device_is_not_dangerous() {
        assert_ne!(classify("dd if=a.img of=b.img"), BashRisk::Dangerous);
    }
}
