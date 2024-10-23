use libium::HOME;
use std::{
    io::Result,
    path::{Path, PathBuf},
};

#[cfg(feature = "gui")]
/// Использует системный выбор файлов для выбора папки с указанным `default` путем
fn show_folder_picker(default: impl AsRef<Path>, prompt: impl Into<String>) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_can_create_directories(true)
        .set_directory(default)
        .set_title(prompt)
        .pick_folder()
}

#[cfg(not(feature = "gui"))]
/// Использует ввод в терминале для выбора папки с указанным `default` путем
fn show_folder_picker(default: impl AsRef<Path>, prompt: impl Into<String>) -> Option<PathBuf> {
    inquire::Text::new(&prompt.into())
        .with_default(&default.as_ref().display().to_string())
        .prompt()
        .ok()
        .map(Into::into)
}

/// Выбирает папку с использованием терминала или системного выбора файлов (в зависимости от флага `gui`)
///
/// Путь по умолчанию `default` отображается/открывается первым, а `name` — это название папки, которую должен выбрать пользователь (например, директория вывода)
pub fn pick_folder(
    default: impl AsRef<Path>,
    prompt: impl Into<String>,
    name: impl AsRef<str>,
) -> Result<Option<PathBuf>> {
    show_folder_picker(default, prompt)
        .map(|raw_in| {
            let path = raw_in
                .components()
                .map(|c| {
                    if c.as_os_str() == "~" {
                        HOME.as_os_str()
                    } else {
                        c.as_os_str()
                    }
                })
                .collect::<PathBuf>()
                .canonicalize()?;

            println!(
                "✔ \x1b[01m{}\x1b[0m · \x1b[32m{}\x1b[0m",
                name.as_ref(),
                path.display(),
            );

            Ok(path)
        })
        .transpose()
}
