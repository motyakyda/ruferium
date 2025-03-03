#![expect(clippy::expect_used, reason = "Для ядовитых мьютексов")]

use crate::{DEFAULT_PARALLEL_NETWORK, PARALLEL_NETWORK, STYLE_BYTE, TICK};
use anyhow::{anyhow, bail, Error, Result};
use colored::Colorize as _;
use fs_extra::{
    dir::{copy as copy_dir, CopyOptions as DirCopyOptions},
    file::{move_file, CopyOptions as FileCopyOptions},
};
use futures::{stream::FuturesUnordered, StreamExt as _};
use indicatif::ProgressBar;
use libium::{iter_ext::IterExt as _, upgrade::DownloadData};
use std::{
    ffi::OsString,
    fs::{copy, create_dir_all, read_dir, remove_file},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

/// Проверяет данную `directory`
///
/// - Если там есть файлы, которых нет в `to_download` или `to_install`, они будут перемещены в `directory`/.old
/// - Если файл в `to_download` или `to_install` уже там, он будет удалён из соответствующего вектора
/// - Если файл является `.part` файлом или если перемещение не удалось, файл будет удалён
pub async fn clean(
    directory: &Path,
    to_download: &mut Vec<DownloadData>,
    to_install: &mut Vec<(OsString, PathBuf)>,
) -> Result<()> {
    let dupes = find_dupes_by_key(to_download, DownloadData::filename);
    if !dupes.is_empty() {
        println!(
            "{}",
            format!(
                "Предупреждение: {} дублирующих файла(ов) найдено {}. Удалите мод, к которому он принадлежит",
                dupes.len(),
                dupes
                    .into_iter()
                    .map(|i| to_download.swap_remove(i).filename())
                    .display(", ")
            )
            .yellow()
            .bold()
        );
    }
    create_dir_all(directory.join(".old"))?;
    for file in read_dir(directory)? {
        let file = file?;
        // Если это файл
        if file.file_type()?.is_file() {
            let filename = file.file_name();
            let filename = filename.to_string_lossy();
            let filename = filename.as_ref();
            // Если он уже загружен
            if let Some(index) = to_download
                .iter()
                .position(|thing| filename == thing.filename())
            {
                // Не загружать его
                to_download.swap_remove(index);
            // Точно так же, если он уже установлен
            } else if let Some(index) = to_install.iter().position(|thing| filename == thing.0) {
                // Не устанавливать его
                to_install.swap_remove(index);
            // В противном случае, переместить файл в `directory`/.old
            // Если файл является `.part` файлом или если перемещение не удалось, удалить файл
            } else if filename.ends_with("part")
                || move_file(
                    file.path(),
                    directory.join(".old").join(filename),
                    &FileCopyOptions::new(),
                )
                .is_err()
            {
                remove_file(file.path())?;
            }
        }
    }
    Ok(())
}

/// Конструирует вектор `to_install` из `directory`
pub fn read_overrides(directory: &Path) -> Result<Vec<(OsString, PathBuf)>> {
    let mut to_install = Vec::new();
    if directory.exists() {
        for file in read_dir(directory)? {
            let file = file?;
            to_install.push((file.file_name(), file.path()));
        }
    }
    Ok(to_install)
}

/// Загружает и устанавливает файлы в `to_download` и `to_install` в `output_dir`
pub async fn download(
    output_dir: PathBuf,
    to_download: Vec<DownloadData>,
    to_install: Vec<(OsString, PathBuf)>,
) -> Result<()> {
    let progress_bar = Arc::new(Mutex::new(
        ProgressBar::new(
            to_download
                .iter()
                .map(|downloadable| downloadable.length as u64)
                .sum(),
        )
        .with_style(STYLE_BYTE.clone()),
    ));
    progress_bar
        .lock()
        .expect("Мьютекс отравлен")
        .enable_steady_tick(Duration::from_millis(100));
    let mut tasks = FuturesUnordered::new();
    let semaphore = Arc::new(Semaphore::new(
        *PARALLEL_NETWORK.get_or_init(|| DEFAULT_PARALLEL_NETWORK),
    ));
    let client = reqwest::Client::new();

    for downloadable in to_download {
        let semaphore = Arc::clone(&semaphore);
        let progress_bar = Arc::clone(&progress_bar);
        let client = client.clone();
        let output_dir = output_dir.clone();

        tasks.push(async move {
            let _permit = semaphore.acquire_owned().await?;

            let (length, filename) = downloadable
                .download(client, &output_dir, |additional| {
                    progress_bar
                        .lock()
                        .expect("Мьютекс отравлен")
                        .inc(additional as u64);
                })
                .await?;
            progress_bar
                .lock()
                .expect("Мьютекс отравлен")
                .println(format!(
                    "{} Загружено  {:>7}  {}",
                    &*TICK,
                    size::Size::from_bytes(length)
                        .format()
                        .with_base(size::Base::Base10)
                        .to_string(),
                    filename.dimmed(),
                ));
            Ok::<(), Error>(())
        });
    }
    while let Some(res) = tasks.next().await {
        res?;
    }
    Arc::try_unwrap(progress_bar)
        .map_err(|_| anyhow!("Не удалось завершить выполнение потоков"))?
        .into_inner()?
        .finish_and_clear();
    for (name, path) in to_install {
        if path.is_file() {
            copy(path, output_dir.join(&name))?;
        } else if path.is_dir() {
            let mut copy_options = DirCopyOptions::new();
            copy_options.overwrite = true;
            copy_dir(path, &output_dir, &copy_options)?;
        } else {
            bail!("Не удалось определить, является ли устанавливаемое файл или папкой")
        }
        println!(
            "{} Установлено          {}",
            &*TICK,
            name.to_string_lossy().dimmed()
        );
    }

    Ok(())
}

/// Находит дубликаты элементов в `slice`, используя значение, полученное с помощью замыкания `key`
///
/// Возвращает индексы дублирующих элементов в обратном порядке для удобного удаления
fn find_dupes_by_key<T, V, F>(slice: &mut [T], key: F) -> Vec<usize>
where
    V: Eq + Ord,
    F: Fn(&T) -> V,
{
    let mut indices = Vec::new();
    if slice.len() < 2 {
        return indices;
    }
    slice.sort_unstable_by_key(&key);
    for i in 0..(slice.len() - 1) {
        if key(&slice[i]) == key(&slice[i + 1]) {
            indices.push(i);
        }
    }
    indices.reverse();
    indices
}
