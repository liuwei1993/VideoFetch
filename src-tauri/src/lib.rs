mod download;
mod library;
mod settings;
mod site;

use download::StartDownloadArgs;
use settings::Settings;

fn with_root<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&std::path::Path, &Settings) -> Result<T, String>,
) -> Result<T, String> {
    let settings = settings::load_settings(app)?;
    let root = settings::library_root_path(&settings);
    library::ensure_library_root(&root)?;
    f(&root, &settings)
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    settings::load_settings(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), String> {
    let root = settings::library_root_path(&settings);
    library::ensure_library_root(&root)?;
    settings::save_settings(&app, &settings)
}

#[tauri::command]
fn ensure_library(app: tauri::AppHandle) -> Result<(), String> {
    with_root(&app, |root, _| library::ensure_library_root(root))
}

#[tauri::command]
fn list_categories(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    with_root(&app, |root, _| library::list_categories(root))
}

#[tauri::command]
fn create_category(app: tauri::AppHandle, name: String) -> Result<(), String> {
    with_root(&app, |root, _| library::create_category(root, &name))
}

#[tauri::command]
fn rename_category(app: tauri::AppHandle, from: String, to: String) -> Result<(), String> {
    with_root(&app, |root, _| library::rename_category(root, &from, &to))
}

#[tauri::command]
fn delete_category(app: tauri::AppHandle, name: String, force: bool) -> Result<(), String> {
    with_root(&app, |root, _| library::delete_category(root, &name, force))
}

#[tauri::command]
fn list_videos(app: tauri::AppHandle, category: String) -> Result<Vec<library::VideoItem>, String> {
    with_root(&app, |root, _| library::list_videos(root, &category))
}

#[tauri::command]
fn move_video(
    app: tauri::AppHandle,
    from_category: String,
    to_category: String,
    filename: String,
) -> Result<(), String> {
    with_root(&app, |root, _| {
        library::move_video(root, &from_category, &to_category, &filename)
    })
}

#[tauri::command]
fn delete_video(app: tauri::AppHandle, category: String, filename: String) -> Result<(), String> {
    with_root(&app, |root, _| library::delete_video(root, &category, &filename))
}

#[tauri::command]
fn open_video(path: String) -> Result<(), String> {
    library::open_video(std::path::Path::new(&path))
}

#[tauri::command]
fn start_download(app: tauri::AppHandle, args: StartDownloadArgs) -> Result<(), String> {
    download::start_download(app, args)
}

#[tauri::command]
fn stop_download() -> Result<(), String> {
    download::stop_download()
}

#[tauri::command]
fn download_running() -> bool {
    download::is_download_running()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let _ = with_root(&handle, |root, _| library::ensure_library_root(root));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            ensure_library,
            list_categories,
            create_category,
            rename_category,
            delete_category,
            list_videos,
            move_video,
            delete_video,
            open_video,
            start_download,
            stop_download,
            download_running,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
