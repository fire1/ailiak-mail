//! Native file pickers that stay safe alongside wry/WebKitGTK on Linux.
//!
//! Opening an in-process GTK/`rfd` dialog while a WebKit child view is alive
//! triggers `Gdk-CRITICAL: _gdk_frame_clock_freeze` / hard crashes. Prefer
//! out-of-process dialogs (zenity / kdialog); fall back to `rfd` elsewhere.

use std::path::PathBuf;
use std::process::Command;

/// One named filter: extensions without a leading dot (`"png"`).
pub struct Filter<'a> {
    pub name: &'a str,
    pub extensions: &'a [&'a str],
}

/// Single-file open dialog.
pub fn pick_file(title: &str, filters: &[Filter<'_>]) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        linux_pick(title, filters, false)?.into_iter().next()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut dlg = rfd::FileDialog::new().set_title(title);
        for f in filters {
            dlg = dlg.add_filter(f.name, f.extensions);
        }
        dlg.pick_file()
    }
}

/// Multi-file open dialog.
pub fn pick_files(title: &str, filters: &[Filter<'_>]) -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        linux_pick(title, filters, true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut dlg = rfd::FileDialog::new().set_title(title);
        for f in filters {
            dlg = dlg.add_filter(f.name, f.extensions);
        }
        dlg.pick_files()
    }
}

/// `Ok(None)` = user cancelled. `Err(())` = helper binary missing.
#[cfg(target_os = "linux")]
type ExternalPick = Result<Option<Vec<PathBuf>>, ()>;

#[cfg(target_os = "linux")]
fn linux_pick(title: &str, filters: &[Filter<'_>], multiple: bool) -> Option<Vec<PathBuf>> {
    match zenity_pick(title, filters, multiple) {
        Ok(paths) => return paths,
        Err(()) => {}
    }
    match kdialog_pick(title, filters, multiple) {
        Ok(paths) => return paths,
        Err(()) => {}
    }
    // Last resort — can CRITICAL with an active WebKit child; install zenity if so.
    let mut dlg = rfd::FileDialog::new().set_title(title);
    for f in filters {
        dlg = dlg.add_filter(f.name, f.extensions);
    }
    if multiple {
        dlg.pick_files()
    } else {
        dlg.pick_file().map(|p| vec![p])
    }
}

#[cfg(target_os = "linux")]
fn zenity_pick(title: &str, filters: &[Filter<'_>], multiple: bool) -> ExternalPick {
    let mut cmd = Command::new("zenity");
    cmd.arg("--file-selection").arg("--title").arg(title);
    if multiple {
        cmd.arg("--multiple").arg("--separator=\n");
    }
    for f in filters {
        let patterns: Vec<String> = f
            .extensions
            .iter()
            .flat_map(|e| [format!("*.{e}"), format!("*.{}", e.to_ascii_uppercase())])
            .collect();
        if patterns.is_empty() {
            continue;
        }
        cmd.arg("--file-filter")
            .arg(format!("{} | {}", f.name, patterns.join(" ")));
    }
    cmd.arg("--file-filter").arg("All files | *");

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return Err(()),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    Ok(if paths.is_empty() { None } else { Some(paths) })
}

#[cfg(target_os = "linux")]
fn kdialog_pick(title: &str, filters: &[Filter<'_>], multiple: bool) -> ExternalPick {
    let mut cmd = Command::new("kdialog");
    cmd.arg("--title").arg(title);
    if multiple {
        cmd.arg("--getopenfilename").arg("--multiple");
    } else {
        cmd.arg("--getopenfilename");
    }
    cmd.arg(".");
    let mut filter_parts = Vec::new();
    for f in filters {
        let patterns: Vec<String> = f.extensions.iter().map(|e| format!("*.{e}")).collect();
        if !patterns.is_empty() {
            filter_parts.push(format!("{} ({})", f.name, patterns.join(" ")));
        }
    }
    if !filter_parts.is_empty() {
        cmd.arg(filter_parts.join("\n"));
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return Err(()),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = if multiple {
        shell_split_paths(stdout.trim())
    } else {
        let t = stdout.trim();
        if t.is_empty() {
            Vec::new()
        } else {
            vec![PathBuf::from(t)]
        }
    };
    Ok(if paths.is_empty() { None } else { Some(paths) })
}

#[cfg(target_os = "linux")]
fn shell_split_paths(s: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in s.chars() {
        match c {
            '"' => in_quote = !in_quote,
            ' ' if !in_quote => {
                if !cur.is_empty() {
                    out.push(PathBuf::from(std::mem::take(&mut cur)));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(PathBuf::from(cur));
    }
    out
}
