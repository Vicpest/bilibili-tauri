import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import './style.css';

const $ = (id) => document.getElementById(id);
const player = $('preview');
let videoInfo = null;
let videoPath = '';
let saveDirectory = '';
let busy = false;
let activeTask = null;
let scrubbing = false;
let previewGeneration = 0;

function status(message, isError = false) {
  $('status').textContent = message;
  $('status').style.color = isError ? '#c43648' : '';
}

function frameStatus(message, isError = false) {
  $('frame-status').textContent = message;
  $('frame-status').style.color = isError ? '#c43648' : '';
}

function refresh() {
  $('parse').disabled = busy;
  $('url').disabled = busy;
  $('quality').disabled = busy || !videoInfo;
  $('download').disabled = busy || !videoInfo || !saveDirectory;
  $('browse').disabled = busy;
  $('reset-directory').disabled = busy;
  $('ffmpeg-browse').disabled = busy;
  $('ffmpeg-reset').disabled = busy;
  $('frame-browse').disabled = busy;
  const playable = Boolean(videoPath) && Number.isFinite(player.duration) && player.duration > 0;
  $('timeline').disabled = !playable;
  $('play-toggle').disabled = !playable;
  $('step-back').disabled = !playable;
  $('step-forward').disabled = !playable;
  $('capture').disabled = busy || !playable || player.seeking;
}

function setPage(page) {
  for (const button of document.querySelectorAll('.nav-item')) {
    const active = button.dataset.page === page;
    button.classList.toggle('active', active);
    if (active) button.setAttribute('aria-current', 'page');
    else button.removeAttribute('aria-current');
  }
  $('page-download').classList.toggle('hidden', page !== 'download');
  $('page-frame').classList.toggle('hidden', page !== 'frame');
  if (page !== 'frame') player.pause();
}

function showSettings(show) {
  $('settings-backdrop').classList.toggle('hidden', !show);
  $('settings-panel').classList.toggle('hidden', !show);
  if (show) $('settings-close').focus();
  else $('settings-open').focus();
}

function updateDirectory(path) {
  saveDirectory = path;
  $('directory').value = path;
  $('frame-location').textContent = path || '请在设置中选择保存目录';
  $('download-location').textContent = path ? `保存至 ${path.split(/[\\/]/).filter(Boolean).pop()}` : '尚未设置目录';
  $('download-location').title = path;
  refresh();
}

function timecode(value, precise = false) {
  if (!Number.isFinite(value)) return precise ? '00:00.000' : '00:00';
  const total = Math.max(0, value);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor(total / 60) % 60;
  const seconds = Math.floor(total) % 60;
  const prefix = hours ? `${String(hours).padStart(2, '0')}:` : '';
  const base = `${prefix}${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  return precise ? `${base}.${String(Math.floor((total % 1) * 1000)).padStart(3, '0')}` : base;
}

function updatePlayback() {
  const time = Number.isFinite(player.currentTime) ? player.currentTime : 0;
  $('current-time').textContent = timecode(time);
  $('exact-time').textContent = timecode(time, true);
  if (!scrubbing) $('timeline').value = String(time);
  $('play-icon').setAttribute('href', player.paused ? '#i-play' : '#i-pause');
}

async function selectLocalVideo(path) {
  const generation = ++previewGeneration;
  player.pause();
  player.removeAttribute('src');
  player.load();
  videoPath = '';
  $('frame-file-name').textContent = '正在打开视频…';
  $('player-empty').classList.remove('hidden');
  frameStatus('');
  refresh();
  const allowedPath = await invoke('prepare_video_preview', { path });
  if (generation !== previewGeneration) return;
  videoPath = allowedPath;
  $('frame-file-name').textContent = allowedPath.split(/[\\/]/).pop();
  $('player-empty').classList.add('hidden');
  player.src = convertFileSrc(allowedPath);
  player.load();
}

for (const button of document.querySelectorAll('.nav-item')) {
  button.addEventListener('click', () => setPage(button.dataset.page));
}
$('settings-open').addEventListener('click', () => showSettings(true));
$('settings-close').addEventListener('click', () => showSettings(false));
$('settings-backdrop').addEventListener('click', () => showSettings(false));
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && !$('settings-panel').classList.contains('hidden')) showSettings(false);
});

$('url').addEventListener('input', () => {
  videoInfo = null;
  $('video').classList.add('hidden');
  $('quality').replaceChildren(new Option('请先解析视频', ''));
  refresh();
});

$('parse').addEventListener('click', async () => {
  busy = true;
  videoInfo = null;
  $('video').classList.add('hidden');
  $('quality').replaceChildren(new Option('正在解析…', ''));
  refresh();
  status('正在解析视频信息…');
  try {
    const info = await invoke('parse_video', { input: $('url').value.trim() });
    videoInfo = info;
    $('video').classList.remove('hidden');
    $('title').textContent = info.title;
    $('video-id').textContent = info.id;
    $('cover').classList.toggle('hidden', !info.thumbnail);
    $('cover-fallback').classList.toggle('hidden', Boolean(info.thumbnail));
    if (info.thumbnail) $('cover').src = info.thumbnail;
    $('quality').replaceChildren(...info.qualities.map((item) => new Option(item.label, String(item.height))));
    status('解析完成，可选择清晰度开始下载');
  } catch (error) {
    status(String(error), true);
  } finally {
    busy = false;
    refresh();
  }
});

$('browse').addEventListener('click', async () => {
  try {
    const directory = await open({ directory: true, multiple: false, title: '选择保存目录' });
    if (typeof directory === 'string') {
      localStorage.setItem('saveDirectory', directory);
      updateDirectory(directory);
    }
  } catch (error) {
    status(`选择目录失败：${error}`, true);
  }
});

$('reset-directory').addEventListener('click', async () => {
  try {
    const directory = await invoke('default_download_directory');
    localStorage.removeItem('saveDirectory');
    updateDirectory(directory);
  } catch (error) {
    frameStatus(String(error), true);
  }
});

$('ffmpeg-browse').addEventListener('click', async () => {
  try {
    const file = await open({ multiple: false, filters: [{ name: 'FFmpeg 程序', extensions: ['exe'] }] });
    if (typeof file === 'string') {
      $('ffmpeg-file').value = file;
      localStorage.setItem('ffmpegPath', file);
    }
  } catch (error) {
    frameStatus(`选择 FFmpeg 失败：${error}`, true);
  }
});
$('ffmpeg-reset').addEventListener('click', () => {
  $('ffmpeg-file').value = '';
  localStorage.removeItem('ffmpegPath');
});

$('download').addEventListener('click', async () => {
  if (!videoInfo || busy) return;
  busy = true;
  activeTask = crypto.randomUUID();
  $('progress').value = 0;
  $('progress-value').textContent = '0%';
  $('speed').textContent = '';
  status('正在准备下载…');
  refresh();
  try {
    const savedPath = await invoke('download_video', {
      request: {
        url: videoInfo.url,
        height: Number($('quality').value),
        directory: saveDirectory,
        taskId: activeTask,
        ffmpegPath: $('ffmpeg-file').value || null,
      },
    });
    if (savedPath) selectLocalVideo(savedPath).catch((error) => frameStatus(String(error), true));
  } catch (error) {
    status(String(error), true);
  } finally {
    activeTask = null;
    busy = false;
    refresh();
  }
});

$('frame-browse').addEventListener('click', async () => {
  try {
    const file = await open({ multiple: false, filters: [{ name: '视频文件', extensions: ['mp4', 'mkv', 'mov', 'webm', 'avi'] }] });
    if (typeof file === 'string') await selectLocalVideo(file);
  } catch (error) {
    frameStatus(`打开视频失败：${error}`, true);
    $('frame-file-name').textContent = '尚未选择视频';
  }
});

player.addEventListener('loadedmetadata', () => {
  $('timeline').max = String(player.duration || 0);
  $('duration').textContent = timecode(player.duration);
  updatePlayback();
  refresh();
});
player.addEventListener('loadeddata', refresh);
player.addEventListener('timeupdate', updatePlayback);
player.addEventListener('play', updatePlayback);
player.addEventListener('pause', updatePlayback);
player.addEventListener('seeking', refresh);
player.addEventListener('seeked', () => { updatePlayback(); refresh(); });
player.addEventListener('error', () => {
  if (videoPath) frameStatus('视频无法预览。请尝试 MP4/H.264 格式的视频。', true);
  refresh();
});
$('play-toggle').addEventListener('click', async () => {
  if (player.paused) {
    try { await player.play(); } catch (error) { frameStatus(`播放失败：${error}`, true); }
  } else player.pause();
});

const timeline = $('timeline');
timeline.addEventListener('pointerdown', () => { scrubbing = true; player.pause(); });
timeline.addEventListener('input', () => {
  const seconds = Number(timeline.value);
  $('current-time').textContent = timecode(seconds);
  $('exact-time').textContent = timecode(seconds, true);
  if (Number.isFinite(seconds)) player.currentTime = seconds;
});
const endScrub = () => { scrubbing = false; updatePlayback(); };
timeline.addEventListener('pointerup', endScrub);
timeline.addEventListener('change', endScrub);
for (const [id, delta] of [['step-back', -0.04], ['step-forward', 0.04]]) {
  $(id).addEventListener('click', () => {
    player.pause();
    player.currentTime = Math.min(player.duration, Math.max(0, player.currentTime + delta));
  });
}

$('capture').addEventListener('click', async () => {
  if (busy || !videoPath || player.seeking) return;
  player.pause();
  busy = true;
  refresh();
  frameStatus('正在保存当前画面…');
  try {
    const path = await invoke('capture_frame', {
      request: {
        videoPath,
        seconds: player.currentTime,
        directory: saveDirectory,
        ffmpegPath: $('ffmpeg-file').value || null,
      },
    });
    frameStatus(`截图已保存：${path}`);
  } catch (error) {
    frameStatus(String(error), true);
  } finally {
    busy = false;
    refresh();
  }
});

listen('download-progress', ({ payload }) => {
  if (payload.taskId !== activeTask) return;
  $('progress').value = payload.percent;
  $('progress-value').textContent = `${Math.round(payload.percent)}%`;
  $('speed').textContent = payload.speed || '';
  status(payload.message, payload.state === 'error');
});

$('ffmpeg-file').value = localStorage.getItem('ffmpegPath') || '';
invoke('default_download_directory').then((defaultDirectory) => {
  updateDirectory(localStorage.getItem('saveDirectory') || defaultDirectory);
}).catch((error) => {
  updateDirectory('');
  status(String(error), true);
});
refresh();
