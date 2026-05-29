mod ncm;
mod convert;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;
use uuid::Uuid;

/// --- State ---

#[derive(Debug, Clone, Serialize)]
pub struct TaskInfo {
    pub status: String,
    pub filename: String,
    pub result: Option<TaskResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskResult {
    pub output: Option<String>,
    pub converted: bool,
    pub deleted: bool,
    pub error: Option<String>,
}

type TaskMap = std::sync::Arc<Mutex<HashMap<String, TaskInfo>>>;

/// --- Request / Response types ---

#[derive(Debug, Deserialize)]
struct EntryItem {
    #[serde(rename = "type")]
    entry_type: String,
    path: Option<String>,
    name: String,
    /// Base64-encoded file data for "file" type entries
    data: Option<String>,
    cinfo: ConvertInfo,
}

#[derive(Debug, Deserialize)]
struct ConvertInfo {
    convert: bool,
    bitrate: u32,
}

#[derive(Debug, Serialize)]
struct DecryptTaskInfo {
    task_id: String,
    filename: String,
}

#[derive(Debug, Serialize)]
struct DecryptResponse {
    tasks: Vec<DecryptTaskInfo>,
}

#[derive(Debug, Serialize)]
struct StatusBatchResponse {
    #[serde(flatten)]
    tasks: HashMap<String, TaskStatusItem>,
}

#[derive(Debug, Serialize)]
struct TaskStatusItem {
    status: String,
    filename: String,
    result: Option<TaskResult>,
}

/// --- Commands ---

#[tauri::command]
async fn browse(app: AppHandle) -> Result<String, String> {
    let (tx, rx) = oneshot::channel();

    app.dialog()
        .file()
        .pick_folder(move |folder| {
            let _ = tx.send(folder);
        });

    let folder = rx.await.map_err(|_| "内部错误".to_string())?;

    match folder {
        Some(path) => Ok(path.to_string()),
        None => Err("用户取消选择".to_string()),
    }
}

#[tauri::command(rename_all = "camelCase")]
async fn decrypt(
    tasks_map: tauri::State<'_, TaskMap>,
    output_dir: String,
    delete_source: bool,
    entries: Vec<EntryItem>,
) -> Result<DecryptResponse, String> {
    // Validate output directory
    let out_path = Path::new(&output_dir);
    if output_dir.trim().is_empty() {
        return Err("请指定输出目录".to_string());
    }
    if !out_path.is_dir() {
        return Err("输出目录不存在".to_string());
    }

    // Temp directory for file-upload type entries
    let temp_dir = std::env::temp_dir().join("music-convert");
    fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;

    // Process entries: validate and prepare
    let mut prepared: Vec<PreparedEntry> = Vec::new();

    for item in &entries {
        let name_lower = item.name.to_lowercase();
        let is_ncm = name_lower.ends_with(".ncm");
        let is_convertible = convert::can_convert(&item.name);

        if !is_ncm && !is_convertible {
            continue;
        }

        match item.entry_type.as_str() {
            "path" => {
                let p = item.path.as_deref().unwrap_or("").trim();
                if p.is_empty() || !Path::new(p).is_file() {
                    continue;
                }
                prepared.push(PreparedEntry {
                    source_path: p.to_string(),
                    is_tmp: false,
                    name: item.name.clone(),
                    convert: item.cinfo.convert && item.cinfo.bitrate > 0,
                    bitrate: item.cinfo.bitrate,
                });
            }
            "file" => {
                let file_data = match &item.data {
                    Some(b64) => base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .map_err(|e| format!("Base64 解码失败: {}", e))?,
                    None => continue,
                };

                let safe_name = sanitize_filename(&item.name);
                let tmp_path = temp_dir.join(format!("{}_{}", Uuid::new_v4(), safe_name));
                let mut f = fs::File::create(&tmp_path)
                    .map_err(|e| format!("创建临时文件失败: {}", e))?;
                f.write_all(&file_data)
                    .map_err(|e| format!("写入临时文件失败: {}", e))?;

                prepared.push(PreparedEntry {
                    source_path: tmp_path.to_string_lossy().to_string(),
                    is_tmp: true,
                    name: item.name.clone(),
                    convert: item.cinfo.convert && item.cinfo.bitrate > 0,
                    bitrate: item.cinfo.bitrate,
                });
            }
            _ => continue,
        }
    }

    if prepared.is_empty() {
        return Err("未找到有效的音频文件".to_string());
    }

    eprintln!("[DEBUG] decrypt: {} entries, delete_source={delete_source}", prepared.len());

    // Create tasks and spawn processing
    let mut task_infos: Vec<DecryptTaskInfo> = Vec::new();
    let output_dir_owned = output_dir.clone();

    for entry in prepared {
        let tid = Uuid::new_v4().to_string().replace("-", "");
        let filename = entry.name.clone();

        {
            let mut tasks = tasks_map.lock().unwrap();
            tasks.insert(
                tid.clone(),
                TaskInfo {
                    status: "queued".to_string(),
                    filename: filename.clone(),
                    result: None,
                },
            );
        }

        task_infos.push(DecryptTaskInfo {
            task_id: tid.clone(),
            filename,
        });

        let output_dir_clone = output_dir_owned.clone();
        let tasks_clone = tasks_map.inner().clone();

        // Spawn a blocking task for CPU-intensive work
        tokio::task::spawn_blocking(move || {
            process_entry(tid, entry, &output_dir_clone, delete_source, &tasks_clone);
        });
    }

    Ok(DecryptResponse { tasks: task_infos })
}

#[tauri::command(rename_all = "camelCase")]
fn status_batch(
    tasks_map: tauri::State<'_, TaskMap>,
    task_ids: Vec<String>,
) -> StatusBatchResponse {
    let tasks = tasks_map.lock().unwrap();
    let mut results: HashMap<String, TaskStatusItem> = HashMap::new();

    for tid in &task_ids {
        if let Some(task) = tasks.get(tid) {
            results.insert(
                tid.clone(),
                TaskStatusItem {
                    status: task.status.clone(),
                    filename: task.filename.clone(),
                    result: task.result.clone(),
                },
            );
        }
    }

    StatusBatchResponse { tasks: results }
}

/// --- Internal types & functions ---

struct PreparedEntry {
    source_path: String,
    is_tmp: bool,
    name: String,
    convert: bool,
    bitrate: u32,
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn process_entry(
    task_id: String,
    entry: PreparedEntry,
    output_dir: &str,
    delete_source: bool,
    tasks_map: &TaskMap,
) {
    // Update status to processing
    {
        let mut tasks = tasks_map.lock().unwrap();
        if let Some(task) = tasks.get_mut(&task_id) {
            task.status = "processing".to_string();
        }
    }

    eprintln!("[DEBUG] process_entry: source={}, delete_source={delete_source}", entry.source_path);

    let result = do_process_entry(&entry, output_dir, delete_source);

    // Update task with result
    {
        let mut tasks = tasks_map.lock().unwrap();
        if let Some(task) = tasks.get_mut(&task_id) {
            match result {
                Ok((output, converted, deleted)) => {
                    task.status = "done".to_string();
                    task.result = Some(TaskResult {
                        output: Some(output),
                        converted,
                        deleted,
                        error: None,
                    });
                }
                Err(e) => {
                    task.status = "error".to_string();
                    task.result = Some(TaskResult {
                        output: None,
                        converted: false,
                        deleted: false,
                        error: Some(e),
                    });
                }
            }
        }
    }

    // Clean up temp files
    if entry.is_tmp {
        if Path::new(&entry.source_path).is_file() {
            let _ = fs::remove_file(&entry.source_path);
        }
    }
}

fn do_process_entry(
    entry: &PreparedEntry,
    output_dir: &str,
    delete_source: bool,
) -> Result<(String, bool, bool), String> {
    let source_path = &entry.source_path;
    let name = &entry.name;
    let is_ncm = name.to_lowercase().ends_with(".ncm");
    let do_convert = entry.convert && entry.bitrate > 0;

    if is_ncm {
        // Decrypt NCM
        let dec_result =
            ncm::decrypt_ncm(source_path, output_dir)?;
        let final_path = dec_result.audio_file;

        // Optional bitrate conversion
        if do_convert {
            convert::convert_bitrate(&final_path, &final_path, entry.bitrate)?;
        }

        let deleted =
            maybe_delete_source(source_path, &final_path, delete_source);

        Ok((final_path, do_convert, deleted))
    } else {
        // Regular audio conversion
        let final_path = Path::new(output_dir)
            .join(name)
            .to_string_lossy()
            .to_string();

        convert::convert_bitrate(
            source_path,
            &final_path,
            if do_convert { entry.bitrate } else { 0 },
        )?;

        let deleted =
            maybe_delete_source(source_path, &final_path, delete_source);

        Ok((final_path, do_convert, deleted))
    }
}

fn maybe_delete_source(source_path: &str, output_path: &str, delete_source: bool) -> bool {
    eprintln!("[DEBUG] maybe_delete_source: path={source_path}, delete_source={delete_source}");
    if !delete_source {
        eprintln!("[DEBUG] maybe_delete_source: skipped (delete_source=false)");
        return false;
    }
    let src = Path::new(source_path);
    if !src.is_file() {
        return false;
    }
    // Don't delete if source == output
    if let (Ok(canon_src), Ok(canon_out)) =
        (fs::canonicalize(source_path), fs::canonicalize(output_path))
    {
        if canon_src == canon_out {
            return false;
        }
    }
    fs::remove_file(source_path).is_ok()
}

/// --- Entry Point ---

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(TaskMap::default())
        .invoke_handler(tauri::generate_handler![
            browse,
            decrypt,
            status_batch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
