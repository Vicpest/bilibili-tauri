use std::{
    collections::BTreeSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use url::Url;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Quality {
    height: u32,
    label: String,
}

#[derive(Serialize)]
struct VideoInfo {
    url: String,
    id: String,
    title: String,
    thumbnail: Option<String>,
    qualities: Vec<Quality>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadRequest {
    url: String,
    height: u32,
    directory: String,
    task_id: String,
    ffmpeg_path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameRequest {
    video_path: String,
    seconds: f64,
    directory: String,
    ffmpeg_path: Option<String>,
}

fn configured_ffmpeg(path: Option<&str>) -> Result<&str, String> {
    let executable = path.filter(|path| !path.is_empty()).unwrap_or("ffmpeg");
    if executable != "ffmpeg" && !Path::new(executable).is_file() {
        return Err("设置中的 FFmpeg 程序不存在，请重新选择".into());
    }
    Ok(executable)
}

#[tauri::command]
fn default_download_directory(app: tauri::AppHandle) -> Result<String, String> {
    let beside_exe = std::env::current_exe()
        .map_err(|e| format!("无法定位程序目录：{e}"))?
        .parent()
        .ok_or("无法定位程序目录")?
        .join("download");
    let directory = if std::fs::create_dir_all(&beside_exe).is_ok() {
        beside_exe
    } else {
        let fallback = app
            .path()
            .app_local_data_dir()
            .map_err(|e| format!("无法定位应用数据目录：{e}"))?
            .join("download");
        std::fs::create_dir_all(&fallback).map_err(|e| format!("无法创建保存目录：{e}"))?;
        fallback
    };
    Ok(directory.to_string_lossy().into_owned())
}

#[tauri::command]
fn prepare_video_preview(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let canonical = Path::new(&path)
        .canonicalize()
        .map_err(|e| format!("无法打开视频：{e}"))?;
    if !canonical.is_file() {
        return Err("所选视频文件不存在".into());
    }
    app.asset_protocol_scope()
        .allow_file(&canonical)
        .map_err(|e| format!("无法授权视频预览：{e}"))?;
    Ok(canonical.to_string_lossy().into_owned())
}

fn hidden_command(name: &str) -> Command {
    let mut command = Command::new(name);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    command
}

async fn thumbnail_data(url: &str) -> Option<String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("bilibili-cover-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&directory).ok()?;
    let template = directory.join("cover.%(ext)s");
    let output = hidden_command("yt-dlp")
        .args([
            "--ignore-config",
            "--no-playlist",
            "--skip-download",
            "--write-thumbnail",
            "-o",
        ])
        .arg(&template)
        .args(["--", url])
        .output()
        .await;
    let image = if output.is_ok_and(|result| result.status.success()) {
        std::fs::read_dir(&directory).ok().and_then(|entries| {
            entries.filter_map(Result::ok).find_map(|entry| {
                let path = entry.path();
                let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                    "jpg" | "jpeg" => "image/jpeg",
                    "png" => "image/png",
                    "webp" => "image/webp",
                    _ => return None,
                };
                let bytes = std::fs::read(path).ok()?;
                if bytes.len() > 5_000_000 {
                    return None;
                }
                Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
            })
        })
    } else {
        None
    };
    let _ = std::fs::remove_dir_all(directory);
    image
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    task_id: String,
    percent: f64,
    speed: String,
    state: String,
    message: String,
}

fn video_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("请输入 Bilibili 视频链接或 BV / AV 号".into());
    }

    let upper = input.to_ascii_uppercase();
    if upper.starts_with("BV")
        && input.len() == 12
        && input[2..].chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Ok(format!("https://www.bilibili.com/video/{input}"));
    }
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("av") && input.len() > 2 && input[2..].chars().all(|c| c.is_ascii_digit())
    {
        return Ok(format!("https://www.bilibili.com/video/{input}"));
    }

    let parsed = Url::parse(input).map_err(|_| "链接格式无效".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("只支持 HTTP 或 HTTPS 视频链接".into());
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if host == "b23.tv" {
        return Ok(parsed.to_string());
    }
    if !matches!(
        host.as_str(),
        "www.bilibili.com" | "m.bilibili.com" | "bilibili.com"
    ) {
        return Err("请输入 Bilibili 视频页面链接".into());
    }
    let mut segments = parsed.path_segments().ok_or("视频页面路径无效")?;
    if segments.next() != Some("video") {
        return Err("请输入 /video/BV... 或 /video/av... 页面链接".into());
    }
    let id = segments.next().unwrap_or_default();
    let id_upper = id.to_ascii_uppercase();
    let id_lower = id.to_ascii_lowercase();
    let valid_bv = id_upper.starts_with("BV")
        && id.len() == 12
        && id[2..].chars().all(|c| c.is_ascii_alphanumeric());
    let valid_av =
        id_lower.starts_with("av") && id.len() > 2 && id[2..].chars().all(|c| c.is_ascii_digit());
    if !valid_bv && !valid_av {
        return Err("无法识别视频 BV / AV 号".into());
    }
    Ok(parsed.to_string())
}

async fn check_tool(name: &str) -> Result<(), String> {
    let is_ffmpeg = Path::new(name)
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("ffmpeg"));
    let version_arg = if is_ffmpeg { "-version" } else { "--version" };
    let result = hidden_command(name).arg(version_arg).output().await;
    match result {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(format!(
            "找不到可用的 {name}，请在设置中选择程序或将其加入 PATH 后重启"
        )),
    }
}

#[tauri::command]
async fn parse_video(input: String) -> Result<VideoInfo, String> {
    let url = video_url(&input)?;
    check_tool("yt-dlp").await?;
    let output = hidden_command("yt-dlp")
        .args([
            "--ignore-config",
            "--encoding",
            "utf-8",
            "--no-playlist",
            "--dump-single-json",
            "--skip-download",
            "--",
            &url,
        ])
        .output()
        .await
        .map_err(|e| format!("启动 yt-dlp 失败：{e}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!("解析失败：{}", detail.trim()));
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("yt-dlp 返回的视频信息无效：{e}"))?;
    let mut heights = BTreeSet::new();
    if let Some(formats) = value["formats"].as_array() {
        for format in formats {
            if format["vcodec"]
                .as_str()
                .is_some_and(|codec| codec != "none")
            {
                if let Some(height) = format["height"]
                    .as_u64()
                    .and_then(|h| u32::try_from(h).ok())
                {
                    heights.insert(height);
                }
            }
        }
    }
    if heights.is_empty() {
        if let Some(height) = value["height"].as_u64().and_then(|h| u32::try_from(h).ok()) {
            heights.insert(height);
        }
    }
    if heights.is_empty() {
        return Err("未找到可下载的视频清晰度；视频可能不可公开访问".into());
    }
    let qualities = heights
        .into_iter()
        .rev()
        .map(|height| Quality {
            height,
            label: format!("{height}p"),
        })
        .collect();

    let thumbnail = thumbnail_data(&url).await;
    Ok(VideoInfo {
        url,
        id: value["id"].as_str().unwrap_or("未知视频").to_owned(),
        title: value["title"].as_str().unwrap_or("未命名视频").to_owned(),
        thumbnail,
        qualities,
    })
}

fn emit(
    app: &tauri::AppHandle,
    task_id: &str,
    percent: f64,
    speed: &str,
    state: &str,
    message: &str,
) {
    let _ = app.emit(
        "download-progress",
        ProgressEvent {
            task_id: task_id.to_owned(),
            percent,
            speed: speed.to_owned(),
            state: state.to_owned(),
            message: message.to_owned(),
        },
    );
}

#[tauri::command]
async fn download_video(
    app: tauri::AppHandle,
    request: DownloadRequest,
) -> Result<Option<String>, String> {
    let url = video_url(&request.url)?;
    if !(144..=4320).contains(&request.height) {
        return Err("清晰度无效，请重新解析视频".into());
    }
    if !Path::new(&request.directory).is_dir() {
        return Err("保存目录不存在，请重新选择".into());
    }
    check_tool("yt-dlp").await?;
    let ffmpeg = configured_ffmpeg(request.ffmpeg_path.as_deref())?;
    check_tool(ffmpeg).await?;

    let selector = format!(
        "bv[height<={}]+ba/b[height<={}]",
        request.height, request.height
    );
    emit(
        &app,
        &request.task_id,
        0.0,
        "",
        "downloading",
        "正在连接并下载…",
    );
    let mut command = hidden_command("yt-dlp");
    command.args([
        "--ignore-config",
        "--encoding",
        "utf-8",
        "--no-playlist",
        "--newline",
        "--progress",
        "--progress-template",
        "download:PROGRESS|%(progress._percent_str)s|%(progress._speed_str)s",
        "--print",
        "after_move:SAVED|%(filepath)s",
        "--merge-output-format",
        "mp4",
        "-f",
        &selector,
        "-P",
        &request.directory,
        "-o",
        "%(title).180B [%(id)s].%(ext)s",
    ]);
    if ffmpeg != "ffmpeg" {
        command.args(["--ffmpeg-location", ffmpeg]);
    }
    let mut child = command
        .args(["--", &url])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("启动 yt-dlp 失败：{e}"))?;

    let stdout = child.stdout.take().ok_or("无法读取下载进度")?;
    let stderr = child.stderr.take().ok_or("无法读取下载错误")?;
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut recent = std::collections::VecDeque::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if recent.len() == 8 {
                recent.pop_front();
            }
            recent.push_back(line);
        }
        recent.into_iter().collect::<Vec<_>>().join("\n")
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut saved_path = None;
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("读取下载进度失败：{e}"))?
    {
        if let Some(rest) = line.strip_prefix("PROGRESS|") {
            let mut parts = rest.splitn(2, '|');
            let percent = parts
                .next()
                .unwrap_or_default()
                .trim()
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap_or(0.0)
                .clamp(0.0, 100.0);
            let speed = parts.next().unwrap_or_default().trim();
            emit(
                &app,
                &request.task_id,
                percent,
                speed,
                "downloading",
                "正在下载音视频…",
            );
        } else if let Some(path) = line.strip_prefix("SAVED|") {
            saved_path = Some(path.to_owned());
        } else if line.contains("Merging formats") || line.contains("[Merger]") {
            emit(
                &app,
                &request.task_id,
                100.0,
                "",
                "merging",
                "正在用 FFmpeg 合并音视频…",
            );
        }
    }

    let result = child
        .wait()
        .await
        .map_err(|e| format!("等待下载进程失败：{e}"))?;
    let errors = stderr_task.await.unwrap_or_default();
    if !result.success() {
        let detail = if errors.trim().is_empty() {
            "下载失败，请检查网络和视频权限"
        } else {
            errors.trim()
        };
        emit(&app, &request.task_id, 0.0, "", "error", detail);
        return Err(detail.to_owned());
    }
    let message = match &saved_path {
        Some(path) => format!("下载完成：{path}"),
        None => format!("下载完成，文件位于：{}", request.directory),
    };
    emit(&app, &request.task_id, 100.0, "", "done", &message);
    Ok(saved_path)
}

#[tauri::command]
async fn capture_frame(request: FrameRequest) -> Result<String, String> {
    let video = Path::new(&request.video_path);
    if !video.is_file() {
        return Err("视频文件不存在，请重新选择".into());
    }
    if !request.seconds.is_finite() || request.seconds < 0.0 {
        return Err("截图时间须为非负秒数".into());
    }
    let directory = Path::new(&request.directory);
    if !directory.is_dir() {
        return Err("截图保存目录不存在".into());
    }
    let stem = video
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("video");
    let stem: String = stem.chars().take(80).collect();
    let millis = (request.seconds * 1000.0).round() as u64;
    let base = format!(
        "{stem}-{:02}-{:02}-{:02}-{:03}",
        millis / 3_600_000,
        (millis / 60_000) % 60,
        (millis / 1000) % 60,
        millis % 1000
    );
    let mut output = directory.join(format!("{base}.png"));
    for index in 1.. {
        if !output.exists() {
            break;
        }
        output = directory.join(format!("{base}-{index}.png"));
    }
    let ffmpeg = configured_ffmpeg(request.ffmpeg_path.as_deref())?;
    check_tool(ffmpeg).await?;
    let result = hidden_command(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-ss",
        ])
        .arg(request.seconds.to_string())
        .arg("-i")
        .arg(video)
        .args(["-frames:v", "1"])
        .arg(&output)
        .output()
        .await
        .map_err(|e| format!("启动 FFmpeg 失败：{e}"))?;
    if !result.status.success() || !output.is_file() {
        let detail = String::from_utf8_lossy(&result.stderr);
        if detail.contains("Unable to find a suitable output format")
            || detail.contains("Requested output format 'image2' is not a suitable output format")
        {
            return Err("当前 FFmpeg 不支持 PNG 图片输出，请在设置中选择完整版 ffmpeg.exe".into());
        }
        return Err(format!("截图失败：{}", detail.trim()));
    }
    Ok(output.to_string_lossy().into_owned())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            default_download_directory,
            prepare_video_preview,
            parse_video,
            download_video,
            capture_frame
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Tauri application");
}

#[cfg(test)]
mod tests {
    use super::video_url;

    #[test]
    fn accepts_video_ids_and_pages() {
        assert!(video_url("BV1xx411c7mD").is_ok());
        assert!(video_url("av170001").is_ok());
        assert!(video_url("https://www.bilibili.com/video/BV1xx411c7mD?p=2").is_ok());
        assert!(video_url("https://b23.tv/abc123").is_ok());
    }

    #[test]
    fn rejects_unrelated_urls() {
        assert!(video_url("https://example.com/video/BV1xx411c7mD").is_err());
        assert!(video_url("https://www.bilibili.com/space/123").is_err());
    }
}
