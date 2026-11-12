//! Windows setup programs: `<app>-<version>-windows-x64-setup.exe`, built with NSIS.
//!
//! A MoneyKeep App that ships a setup program instead of an MSI is installed per machine, into
//! `C:\Program Files\<App>`, and its setup asks for administrator permission itself (it is
//! marked as needing it). A program marked that way cannot be started directly from an account
//! without that permission, so the launcher starts it through Windows' permission prompt, from a
//! PowerShell that itself runs without it ([`script`]), and waits for it:
//!
//! - `/S` runs it silently: no pages, the default options, an update in place over the version
//!   that's there. A fresh install into a chosen folder adds `/D=<folder>`.
//! - Declining the prompt is a cancelled install ([`DECLINED`]), not an error.
//! - Exit code 0 is success; [`APP_OPEN`] means the app was open and didn't close in time ("Close
//!   <App> and try again."); anything else is a failure with its code.
//!
//! An installed copy is found by its uninstall entry (`HKLM\…\Uninstall\<App>`, publisher
//! MoneyKeep Project, `DisplayVersion`, `InstallLocation`), and removed by running the entry's
//! `QuietUninstallString` the same way ([`uninstall_command`]). The NSIS uninstaller hands over to
//! a copy of itself, so the launcher also waits for the entry to go before it says it's removed.

#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;

use keep_launcher_core::protocol::CANCELLED;

use super::msi_batch::{self, DECLINED, LAUNCH_FAILED};

/// What a MoneyKeep App's setup (and uninstaller) exits with when the app is open and was not
/// closed within its wait.
pub(crate) const APP_OPEN: i32 = 2;

/// The setup's arguments: silent, then the install folder for a fresh install into a chosen one.
/// NSIS takes `/D=` unquoted, as the rest of the command line, so it must come last and a path
/// with a quote or a control character is refused.
pub(crate) fn arguments(folder: Option<&Path>) -> Result<String, String> {
    let mut line = String::from("/S");
    if let Some(folder) = folder {
        let text = folder.to_str().ok_or("unusual characters in a path")?;
        if text.is_empty() || text.chars().any(|c| c == '"' || c.is_control()) {
            return Err(format!("couldn't use install folder {text}: the path contains invalid characters"));
        }
        line.push_str(" /D=");
        line.push_str(text);
    }
    Ok(line)
}

/// The PowerShell the launcher runs without elevation: it starts `program` with `arguments`
/// through the permission prompt, waits for it (and the processes it starts), and exits with its
/// exit code. A declined prompt exits [`DECLINED`], any other failure to start [`LAUNCH_FAILED`],
/// as for the Windows Installer's elevated session. The prompt names the setup itself.
pub(crate) fn script(program: &str, arguments: &str) -> String {
    format!(
        "try {{ $run = Start-Process -FilePath {} -ArgumentList {} -Verb RunAs -Wait -PassThru; exit $run.ExitCode }} catch {{ {} }}",
        msi_batch::quoted(program),
        msi_batch::quoted(arguments),
        msi_batch::launch_catch()
    )
}

/// What a setup's exit code means for the install.
pub(crate) fn explain(app: &str, code: Option<i32>) -> Result<(), String> {
    match code {
        Some(0) => Ok(()),
        Some(DECLINED) => Err(CANCELLED.into()),
        Some(APP_OPEN) => Err(format!("Close {app} and try again.")),
        Some(LAUNCH_FAILED) => Err(format!("couldn't start the {app} setup")),
        Some(c) => Err(format!("the {app} setup stopped with error {c}")),
        None => Err(format!("the {app} setup was interrupted")),
    }
}

/// What an uninstaller's exit code means for the removal.
pub(crate) fn explain_uninstall(app: &str, code: Option<i32>) -> Result<(), String> {
    match code {
        Some(0) => Ok(()),
        Some(DECLINED) => Err(CANCELLED.into()),
        Some(APP_OPEN) => Err(format!("Close {app} and try again.")),
        Some(LAUNCH_FAILED) => Err(format!("couldn't start the {app} uninstaller")),
        Some(c) => Err(format!("the {app} uninstaller stopped with error {c}")),
        None => Err(format!("the {app} uninstaller was interrupted")),
    }
}

/// Splits an uninstall entry's command line into the program and its arguments:
/// `"C:\Program Files\App\uninstall.exe" /S`, or the same without quotes. `None` when it names
/// no `.exe`.
pub(crate) fn split_command(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let (program, rest) = match line.strip_prefix('"') {
        Some(quoted) => {
            let end = quoted.find('"')?;
            (quoted.get(..end)?, quoted.get(end + 1..)?)
        }
        None => {
            let end = line.to_ascii_lowercase().find(".exe")?.checked_add(4)?;
            (line.get(..end)?, line.get(end..)?)
        }
    };
    let program = program.trim();
    let ends_in_exe = program.len() > 4 && program.get(program.len() - 4..).is_some_and(|ext| ext.eq_ignore_ascii_case(".exe"));
    if !ends_in_exe || !(rest.is_empty() || rest.starts_with(char::is_whitespace)) || program.chars().any(char::is_control) {
        return None;
    }
    Some((program.to_string(), rest.trim().to_string()))
}

/// The silent uninstall command of an entry: its `QuietUninstallString`, else its
/// `UninstallString` with `/S` added (NSIS's silent switch) when it has none.
pub(crate) fn uninstall_command(quiet: &str, normal: &str) -> Option<(String, String)> {
    if !quiet.trim().is_empty() {
        return split_command(quiet);
    }
    let (program, arguments) = split_command(normal)?;
    if arguments.split_whitespace().any(|a| a.eq_ignore_ascii_case("/S")) {
        return Some((program, arguments));
    }
    Some((program, if arguments.is_empty() { "/S".into() } else { format!("/S {arguments}") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setup_runs_silently_and_takes_a_chosen_folder_last_and_unquoted() {
        assert_eq!(arguments(None).unwrap(), "/S");
        assert_eq!(arguments(Some(Path::new(r"D:\Apps\MoneyKeep"))).unwrap(), r"/S /D=D:\Apps\MoneyKeep");
        // Spaces stay as they are: NSIS reads /D= to the end of the line.
        assert_eq!(arguments(Some(Path::new(r"C:\Program Files\Form1099Keep"))).unwrap(), r"/S /D=C:\Program Files\Form1099Keep");
        for bad in ["C:\\a\"b", "C:\\a\nb", ""] {
            assert!(arguments(Some(Path::new(bad))).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_setup_is_started_through_the_permission_prompt_and_waited_for() {
        let script = script(r"C:\Users\O'Neil\AppData\Local\dl\moneykeep-0.1.0-alpha.1-windows-x64-setup.exe", "/S");
        assert!(
            script.starts_with(
                r"try { $run = Start-Process -FilePath 'C:\Users\O''Neil\AppData\Local\dl\moneykeep-0.1.0-alpha.1-windows-x64-setup.exe'"
            ),
            "{script}"
        );
        assert!(script.contains("-ArgumentList '/S' -Verb RunAs -Wait -PassThru; exit $run.ExitCode"), "{script}");
        // A declined prompt and a failed start are told apart, as for the Windows Installer.
        assert!(script.ends_with(&format!("catch {{ {} }}", msi_batch::launch_catch())), "{script}");
        assert!(script.contains(&format!("exit {DECLINED}")) && script.contains(&format!("exit {LAUNCH_FAILED}")));
        let with_folder = super::script(r"C:\dl\a.exe", r"/S /D=C:\Program Files\CalcKeep");
        assert!(with_folder.contains(r"-ArgumentList '/S /D=C:\Program Files\CalcKeep'"), "{with_folder}");
    }

    #[test]
    fn exit_codes_say_what_happened() {
        assert_eq!(explain("MoneyKeep", Some(0)), Ok(()));
        // A declined permission prompt is a cancelled install, which the UI shows quietly.
        assert_eq!(explain("MoneyKeep", Some(DECLINED)), Err(CANCELLED.to_string()));
        assert_eq!(explain("MoneyKeep", Some(APP_OPEN)), Err("Close MoneyKeep and try again.".to_string()));
        assert_eq!(explain("CalcKeep", Some(1)), Err("the CalcKeep setup stopped with error 1".to_string()));
        assert_eq!(explain("CalcKeep", Some(LAUNCH_FAILED)), Err("couldn't start the CalcKeep setup".to_string()));
        assert_eq!(explain("CalcKeep", None), Err("the CalcKeep setup was interrupted".to_string()));
        assert_eq!(explain_uninstall("GroupKeep", Some(0)), Ok(()));
        assert_eq!(explain_uninstall("GroupKeep", Some(DECLINED)), Err(CANCELLED.to_string()));
        assert_eq!(explain_uninstall("GroupKeep", Some(APP_OPEN)), Err("Close GroupKeep and try again.".to_string()));
        assert_eq!(explain_uninstall("GroupKeep", Some(5)), Err("the GroupKeep uninstaller stopped with error 5".to_string()));
    }

    #[test]
    fn uninstall_commands_are_split_into_program_and_arguments() {
        let quiet = r#""C:\Program Files\MoneyKeep\uninstall.exe" /S"#;
        assert_eq!(split_command(quiet), Some((r"C:\Program Files\MoneyKeep\uninstall.exe".into(), "/S".into())));
        assert_eq!(split_command(r"C:\Apps\CalcKeep\Uninstall.EXE /S"), Some((r"C:\Apps\CalcKeep\Uninstall.EXE".into(), "/S".into())));
        assert_eq!(split_command(r#""C:\Apps\uninstall.exe""#), Some((r"C:\Apps\uninstall.exe".into(), String::new())));
        for bad in ["", "\"", r#""C:\Apps\uninstall"#, r"C:\Apps\uninstall.cmd /S", r"C:\Apps\uninstall.exefoo /S", r#""C:\Apps\uninstall.bat" /S"#] {
            assert_eq!(split_command(bad), None, "{bad:?}");
        }
        // The quiet string first; without one, the normal one made silent.
        assert_eq!(uninstall_command(quiet, r#""C:\x\other.exe""#), Some((r"C:\Program Files\MoneyKeep\uninstall.exe".into(), "/S".into())));
        assert_eq!(uninstall_command("", r#""C:\x\uninstall.exe""#), Some((r"C:\x\uninstall.exe".into(), "/S".into())));
        assert_eq!(uninstall_command("  ", r#""C:\x\uninstall.exe" /s"#), Some((r"C:\x\uninstall.exe".into(), "/s".into())));
        assert_eq!(uninstall_command("", ""), None);
    }
}
