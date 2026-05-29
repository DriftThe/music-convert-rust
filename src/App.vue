<script setup lang="ts">
import { onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core"
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow"
import { listen } from "@tauri-apps/api/event"

// --- Dark mode ---
function initTheme() {
  const saved = localStorage.getItem("theme");
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const theme = saved || (prefersDark ? "dark" : "light");
  document.documentElement.setAttribute("data-theme", theme);
}
function toggleTheme() {
  const current = document.documentElement.getAttribute("data-theme");
  const next = current === "dark" ? "light" : "dark";
  document.documentElement.setAttribute("data-theme", next);
  localStorage.setItem("theme", next);
}

onMounted(() => {
  initTheme();

  (function () {
    var $ = function (s: string) { return document.querySelector(s); };
    var $$ = function (s: string) { return document.querySelectorAll(s); };
    var dropzone = $('#dropzone');
    var fileInput = $('#fileInput') as HTMLInputElement;
    var pathInput = $('#pathInput') as HTMLTextAreaElement;
    var fileList = $('#fileList');
    var fileCount = $('#fileCount');
    var decryptBtn = $('#decryptBtn') as HTMLButtonElement;
    var outputDir = $('#outputDir') as HTMLInputElement;
    var deleteSource = $('#deleteSource') as HTMLInputElement;
    var toastContainer = $('#toastContainer');
    var browseBtn = $('#browseBtn') as HTMLButtonElement;
    var addPathsBtn = $('#addPathsBtn') as HTMLButtonElement;
    var clearBtn = $('#clearBtn') as HTMLButtonElement;
    var clearDoneBtn = $('#clearDoneBtn') as HTMLButtonElement;
    var themeToggleBtn = $('#themeToggleBtn') as HTMLButtonElement;
    var winMinBtn = $('#winMinBtn') as HTMLButtonElement;
    var winMaxBtn = $('#winMaxBtn') as HTMLButtonElement;
    var winCloseBtn = $('#winCloseBtn') as HTMLButtonElement;
    var pendingFiles: any[] = [];
    var taskStatuses: Record<string, string> = {};
    var processing = false;
    var NCM_EXT = '.ncm';
    var CONVERTIBLE = ['.mp3', '.flac', '.wav', '.m4a', '.aac', '.ogg', '.wma', '.ape', '.opus', '.alac', '.aiff', '.wv'];
    function isNcm(name: string) { return name.toLowerCase().endsWith(NCM_EXT); }
    function isConvertible(name: string) { var ln = name.toLowerCase(); return CONVERTIBLE.some(function (ext) { return ln.endsWith(ext); }); }
    function showToast(msg: string, type?: string) {
      type = type || 'info'; var el = document.createElement('div');
      el.className = 'toast ' + type; el.textContent = msg; toastContainer!.appendChild(el);
      el.addEventListener('animationend', function (e) { if ((e as AnimationEvent).animationName === 'toastOut') el.remove(); });
    }
    function addFile(fileObj: any) {
      if (processing) { showToast('正在处理中，请等待完成后再添加文件', 'warn'); return; }
      var name = fileObj.name || ''; var ln = name.toLowerCase();
      if (!ln.endsWith(NCM_EXT) && !isConvertible(name)) { showToast('不支持的文件格式: ' + name, 'warn'); return; }
      if (pendingFiles.some(function (f: any) { return f.name === name; })) return;
      fileObj.isNcm = ln.endsWith(NCM_EXT);
      fileObj.convertMode = fileObj.isNcm ? 'none' : 'medium';
      fileObj.customBitrate = 0;
      pendingFiles.push(fileObj); renderFileList();
    }
    function removeFile(name: string) {
      var idx = pendingFiles.findIndex(function (f: any) { return f.name === name; });
      if (idx >= 0) pendingFiles.splice(idx, 1); renderFileList();
    }
    function getConvertInfo(fileObj: any) {
      if (fileObj.convertMode === 'none' || !fileObj.convertMode) return { convert: false, bitrate: 0 };
      if (fileObj.convertMode === 'custom') return { convert: true, bitrate: Math.max(8, parseInt(fileObj.customBitrate) || 192) };
      var presets: Record<string, number> = { low: 96, medium: 192, high: 320 };
      return { convert: true, bitrate: presets[fileObj.convertMode] || 192 };
    }
    function renderFileList() {
      fileList!.innerHTML = '';
      pendingFiles.forEach(function (f: any, i: number) {
        var saved = taskStatuses[f.name];
        var statusText = '等待中';
        var statusClass = 'pending';
        if (saved) {
          if (saved === 'processing') { statusText = '处理中...'; statusClass = 'processing'; }
          else if (saved === 'done') { statusText = '完成'; statusClass = 'done'; }
          else if (saved === 'error') { statusText = '失败'; statusClass = 'error'; }
        }
        var item = document.createElement('div'); item.className = 'file-item'; item.setAttribute('data-filename', f.name);
        var top = document.createElement('div'); top.className = 'file-item-top';
        top.innerHTML = '<span class="file-icon">&#9835;</span><span class="file-name">' + esc(f.name) + '</span><span class="file-status ' + statusClass + '">' + statusText + '</span><button class="file-remove" data-filename="' + escAttr(f.name) + '">&times;</button>';
        var extra = document.createElement('div'); extra.className = 'file-item-extra';
        if (f.isNcm) {
          var chk = f.convertMode !== 'none';
          extra.innerHTML = '<label><input type="checkbox" class="cb-convert" data-idx="' + i + '"' + (chk ? ' checked' : '') + '> 解密后转换</label><span class="conv-opts" style="display:' + (chk ? 'inline' : 'none') + '">' + buildQualitySelect(i) + '</span>';
        } else {
          extra.innerHTML = buildQualitySelect(i);
        }
        item.appendChild(top); item.appendChild(extra); fileList!.appendChild(item);
      });
      fileCount!.textContent = '共 ' + pendingFiles.length + ' 个文件';
      decryptBtn!.disabled = pendingFiles.length === 0 || processing;
      bindRemoveButtons(); bindConvertCheckboxes(); bindQualitySelects();
    }
    function buildQualitySelect(idx: number) {
      var f = pendingFiles[idx]; var m = f.convertMode || 'medium';
      return '<select class="sel-quality" data-idx="' + idx + '">' +
        '<option value="low"' + (m === 'low' ? ' selected' : '') + '>低 (96kbps)</option>' +
        '<option value="medium"' + (m === 'medium' ? ' selected' : '') + '>中 (192kbps)</option>' +
        '<option value="high"' + (m === 'high' ? ' selected' : '') + '>高 (320kbps)</option>' +
        '<option value="custom"' + (m === 'custom' ? ' selected' : '') + '>自定义</option>' +
        '</select><input type="number" class="inp-bitrate" data-idx="' + idx + '" placeholder="kbps" value="' + (f.customBitrate || '') + '" style="width:72px;display:' + (m === 'custom' ? 'inline-block' : 'none') + '" min="8" max="512">';
    }
    function bindRemoveButtons() {
      $$('.file-remove').forEach(function (btn) { btn.addEventListener('click', function (e) { e.stopPropagation(); removeFile((btn as HTMLElement).dataset.filename!); }); });
    }
    function bindConvertCheckboxes() {
      $$('.cb-convert').forEach(function (cb) { cb.addEventListener('change', function () { var i = parseInt((cb as HTMLElement).dataset.idx!); pendingFiles[i].convertMode = (cb as HTMLInputElement).checked ? 'medium' : 'none'; renderFileList(); }); });
    }
    function bindQualitySelects() {
      $$('.sel-quality').forEach(function (sel) { sel.addEventListener('change', function () { var i = parseInt((sel as HTMLElement).dataset.idx!); pendingFiles[i].convertMode = (sel as HTMLSelectElement).value; renderFileList(); }); });
      $$('.inp-bitrate').forEach(function (inp) { inp.addEventListener('input', function () { var i = parseInt((inp as HTMLElement).dataset.idx!); pendingFiles[i].customBitrate = parseInt((inp as HTMLInputElement).value) || 0; }); });
    }
    function esc(s: string) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
    function escAttr(s: string) { return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/'/g, '&#39;').replace(/</g, '&lt;').replace(/>/g, '&gt;'); }

    // --- Theme toggle ---
    themeToggleBtn!.addEventListener('click', toggleTheme);

    browseBtn!.addEventListener('click', async () => {
      browseBtn!.disabled = true;
      browseBtn!.textContent = '...';
      try {
        const path = await invoke<string>('browse');
        outputDir!.value = path;
      } catch (err: any) {
        showToast('浏览失败: ' + err, 'error');
      } finally {
        browseBtn!.disabled = false;
        browseBtn!.textContent = '浏览...';
      }
    });
    clearBtn!.addEventListener('click', function () { pendingFiles = []; taskStatuses = {}; renderFileList(); });
    clearDoneBtn!.addEventListener('click', function () {
      pendingFiles = pendingFiles.filter(function (f: any) {
        var s = taskStatuses[f.name];
        return !s || (s !== 'done' && s !== 'error');
      });
      renderFileList();
    });
    addPathsBtn!.addEventListener('click', function () {
      var text = pathInput!.value.trim(); if (!text) return;
      text.split(/[\r\n]+/).forEach(function (line) { var p = line.trim(); if (!p) return; var name = p.replace(/\\/g, '/').split('/').pop()!; addFile({ name: name, path: p }); });
      pathInput!.value = '';
    });
    // --- Tauri native drag-drop (gives real file paths) ---
    const appWindow = getCurrentWebviewWindow();

    // Wire up window control buttons
    console.log('[DEBUG] winMinBtn:', winMinBtn, 'winMaxBtn:', winMaxBtn, 'winCloseBtn:', winCloseBtn);
    winMinBtn!.addEventListener('click', function () {
      console.log('[DEBUG] minimize clicked');
      invoke('plugin:window|minimize').catch(function (e) { console.error('minimize error:', e); });
    });
    winMaxBtn!.addEventListener('click', function () {
      console.log('[DEBUG] toggleMaximize clicked');
      invoke('plugin:window|toggle_maximize').catch(function (e) { console.error('toggleMaximize error:', e); });
    });
    winCloseBtn!.addEventListener('click', function () {
      console.log('[DEBUG] close clicked');
      invoke('plugin:window|close').catch(function (e) { console.error('close error:', e); });
    });

    function updateMaxBtnIcon() {
      invoke('plugin:window|is_maximized').then(function (maxed: any) {
        if (maxed) {
          winMaxBtn!.innerHTML = '<svg width="10" height="10" viewBox="0 0 10 10"><rect x="1.5" y="0.5" width="7" height="7" rx="0.5" fill="none" stroke="currentColor" stroke-width="1"/><rect x="0.5" y="2.5" width="7" height="7" rx="0.5" fill="var(--bg-toolbar)" stroke="currentColor" stroke-width="1"/></svg>';
          winMaxBtn!.title = '还原';
        } else {
          winMaxBtn!.innerHTML = '<svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" rx="0.5" fill="none" stroke="currentColor" stroke-width="1"/></svg>';
          winMaxBtn!.title = '最大化';
        }
      });
    }
    updateMaxBtnIcon();
    listen('tauri://resize', function () { updateMaxBtnIcon(); });

    let nativeDropHandled = false;
    appWindow.onDragDropEvent((event) => {
      console.log('[DRAGDROP]', event.payload.type, event.payload);
      if (event.payload.type === 'over') {
        nativeDropHandled = false;
        dropzone!.classList.add('dragover');
      } else if (event.payload.type === 'drop') {
        nativeDropHandled = true;
        dropzone!.classList.remove('dragover');
        for (const path of event.payload.paths) {
          const name = path.replace(/\\/g, '/').split('/').pop()!;
          console.log('[DRAGDROP] file:', path, name);
          if (!isNcm(name) && !isConvertible(name)) {
            showToast('已忽略不支持的文件: ' + name, 'warn');
            continue;
          }
          addFile({ name: name, path: path });
        }
      } else if (event.payload.type === 'leave') {
        dropzone!.classList.remove('dragover');
      }
    });

    // --- HTML5 fallback drag-drop (for when Tauri native events don't fire) ---
    dropzone!.addEventListener('dragover', function (e) { e.preventDefault(); dropzone!.classList.add('dragover'); });
    dropzone!.addEventListener('dragleave', function (e) { e.preventDefault(); dropzone!.classList.remove('dragover'); });
    dropzone!.addEventListener('drop', function (e: any) {
      e.preventDefault(); dropzone!.classList.remove('dragover');
      // Skip if Tauri native handler already processed this drop
      if (nativeDropHandled) { nativeDropHandled = false; return; }
      if (e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files.length > 0) {
        for (var i = 0; i < e.dataTransfer.files.length; i++) {
          var f = e.dataTransfer.files[i];
          if (!isNcm(f.name) && !isConvertible(f.name)) { showToast('已忽略不支持的文件: ' + f.name, 'warn'); continue; }
          addFile({ name: f.name, file: f });
        }
      }
    });

    dropzone!.addEventListener('click', function () { fileInput!.click(); });
    fileInput!.addEventListener('change', function () {
      for (var i = 0; i < fileInput!.files!.length; i++) {
        var f = fileInput!.files![i];
        if (!isNcm(f.name) && !isConvertible(f.name)) { showToast('已忽略不支持的文件: ' + f.name, 'warn'); continue; }
        addFile({ name: f.name, file: f });
      }
      fileInput!.value = '';
    });
    decryptBtn!.addEventListener('click', async function () {
      var outDir = outputDir!.value.trim();
      if (!outDir) { showToast('请先指定输出目录', 'warn'); return; }
      decryptBtn!.disabled = true; decryptBtn!.textContent = '正在提交...';
      processing = true;
      var entryArr: any[] = [];
      // Read file data for 'file' type entries
      for (const f of pendingFiles) {
        if (f.path) {
          entryArr.push({ type: 'path', path: f.path, name: f.name, cinfo: getConvertInfo(f) });
        } else if (f.file) {
          // Convert browser File to base64
          var b64 = await fileToBase64(f.file);
          entryArr.push({ type: 'file', name: f.name, data: b64, cinfo: getConvertInfo(f) });
        }
      }
      try {
        var data: any = await invoke('decrypt', {
          outputDir: outDir,
          deleteSource: deleteSource!.checked,
          entries: entryArr,
        });
        if (data.error) { showToast(data.error, 'error'); decryptBtn!.disabled = false; decryptBtn!.textContent = '开始处理'; processing = false; return; }
        pollTasks(data.tasks);
      } catch (err: any) {
        showToast('提交失败: ' + err, 'error');
        decryptBtn!.disabled = false;
        decryptBtn!.textContent = '开始处理';
        processing = false;
      }
    });

    async function fileToBase64(file: File): Promise<string> {
      return new Promise((resolve, reject) => {
        var reader = new FileReader();
        reader.onload = () => {
          var result = reader.result as string;
          // Remove data:...;base64, prefix
          var comma = result.indexOf(',');
          resolve(comma >= 0 ? result.substring(comma + 1) : result);
        };
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(file);
      });
    }
    function pollTasks(tasks: any[]) {
      var total = tasks.length, errCount = 0, finished = false;
      var taskMap: Record<string, string> = {};
      tasks.forEach(function (t: any) {
        taskMap[t.task_id] = t.filename;
        taskStatuses[t.filename] = 'processing';
      });
      renderFileList();

      var interval = setInterval(async function () {
        var ids = Object.keys(taskMap);
        try {
          var data: any = await invoke('status_batch', { taskIds: ids });
          var allDone = 0;
          ids.forEach(function (tid) {
            var info = data[tid];
            if (!info) return;
            if (info.status === 'done' || info.status === 'error') {
              allDone++;
              taskStatuses[info.filename] = info.status;
              delete taskMap[tid];
              if (info.status === 'error') errCount++;
              var el = getStatusEl(info.filename) as HTMLElement;
              if (el) {
                if (info.status === 'done') { el.className = 'file-status done'; el.textContent = '完成'; }
                else {
                  el.className = 'file-status error'; el.textContent = '失败';
                  var errMsg = (info.result && info.result.error) ? info.result.error : '';
                  if (errMsg) el.title = errMsg;
                }
              }
            }
          });
          checkAll(allDone);
        } catch (_) { }
      }, 500);

      function checkAll(_doneNow: number) {
        if (finished) return;
        var remaining = Object.keys(taskMap).length;
        if (remaining === 0) {
          finished = true;
          clearInterval(interval);
          processing = false;
          decryptBtn!.disabled = pendingFiles.length === 0;
          decryptBtn!.textContent = '开始处理';
          renderFileList();
          if (errCount === 0 && total > 0) showToast('全部处理完成！', 'success');
          else if (total > 0) showToast((total - errCount) + ' 成功, ' + errCount + ' 失败', errCount > 0 ? 'warn' : 'success');
          else showToast('处理失败', 'error');
        }
      }
    }
    function getStatusEl(filename: string) {
      var item = fileList!.querySelector('.file-item[data-filename="' + CSS.escape(filename) + '"]');
      return item ? item.querySelector('.file-status') : null;
    }
  })();
});
</script>

<template>
  <!-- Toast container -->
  <div class="toast-container" id="toastContainer"></div>

  <!-- App shell: full window -->
  <div class="app-shell">
    <!-- Top toolbar -->
    <header class="app-toolbar">
      <div class="toolbar-left">
        <span class="toolbar-icon">&#9835;</span>
        <h1 class="toolbar-title">音频转换工具</h1>
      </div>
      <div class="toolbar-center"></div>
      <div class="toolbar-right">
        <button
          type="button"
          class="btn btn-icon"
          id="themeToggleBtn"
          title="切换深色/浅色模式"
        >
          <span class="icon-sun">&#9728;</span>
          <span class="icon-moon">&#9790;</span>
        </button>
        <div class="win-controls">
          <button type="button" class="win-btn win-btn-min" id="winMinBtn" title="最小化">
            <svg width="10" height="1" viewBox="0 0 10 1"><rect width="10" height="1" fill="currentColor"/></svg>
          </button>
          <button type="button" class="win-btn win-btn-max" id="winMaxBtn" title="最大化">
            <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" rx="0.5" fill="none" stroke="currentColor" stroke-width="1"/></svg>
          </button>
          <button type="button" class="win-btn win-btn-close" id="winCloseBtn" title="关闭">
            <svg width="10" height="10" viewBox="0 0 10 10"><line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" stroke-width="1.2"/><line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1.2"/></svg>
          </button>
        </div>
      </div>
    </header>

    <!-- Main content -->
    <main class="app-main">
      <!-- Left panel: input area -->
      <section class="panel panel-input">
        <div class="panel-section">
          <label class="section-label">输出目录</label>
          <div class="input-group">
            <input type="text" id="outputDir" placeholder="选择或输入输出目录..." />
            <button type="button" class="btn btn-outline" id="browseBtn">浏览...</button>
          </div>
        </div>

        <div class="panel-section">
          <label class="section-label">添加文件路径</label>
          <div class="input-group-col">
            <textarea
              id="pathInput"
              rows="3"
              placeholder="输入 .ncm 或音频文件的完整路径，每行一个&#10;例如: D:\Music\song.ncm"
            ></textarea>
            <button type="button" class="btn btn-outline" id="addPathsBtn">添加</button>
          </div>
        </div>

        <div class="panel-section">
          <div class="dropzone" id="dropzone">
            <div class="dropzone-icon">&#128196;</div>
            <div class="dropzone-title">拖放文件到此处</div>
            <div class="dropzone-sub">或点击选择 .ncm / 音频文件</div>
          </div>
          <input
            type="file"
            id="fileInput"
            accept=".ncm,.mp3,.flac,.wav,.m4a,.aac,.ogg,.wma,.ape,.opus,.alac,.aiff,.wv"
            multiple
            style="display:none;"
          />
        </div>

        <div class="panel-section">
          <label class="checkbox-row">
            <input type="checkbox" id="deleteSource" />
            <span>处理后删除源文件</span>
          </label>
          <div class="delete-hint">注：仅对拖拽上传的来源文件生效</div>
        </div>
      </section>

      <!-- Right panel: file list + actions -->
      <section class="panel panel-list">
        <div class="file-list-header">
          <span class="file-list-count" id="fileCount">共 0 个文件</span>
          <div class="file-list-actions">
            <button type="button" class="btn btn-sm btn-outline" id="clearDoneBtn">清空已完成</button>
            <button type="button" class="btn btn-sm btn-danger" id="clearBtn">清空列表</button>
          </div>
        </div>
        <div class="file-list" id="fileList"></div>
        <div class="panel-footer">
          <button class="btn btn-primary btn-decrypt" id="decryptBtn" disabled>开始处理</button>
        </div>
      </section>
    </main>
  </div>
</template>

<style>
/* ============================================
   CSS Variables: Light & Dark themes
   ============================================ */
:root,
[data-theme="light"] {
  --bg-app: #f5f6f8;
  --bg-toolbar: #ffffff;
  --bg-panel: #ffffff;
  --bg-input: #f3f4f6;
  --bg-hover: #f0f4ff;
  --bg-dropzone: #f8f9fb;
  --bg-dropzone-hover: #eef2ff;
  --bg-file-item: #f9fafb;
  --bg-file-item-hover: #f0f4ff;
  --bg-checkbox: #f3f4f6;
  --bg-checkbox-hover: #eef2ff;
  --bg-warn: #fffbeb;
  --bg-status-pending: #e5e7eb;
  --bg-status-queued: #e0e7ff;
  --bg-status-processing: #dbeafe;
  --bg-status-done: #dcfce7;
  --bg-status-error: #fef2f2;

  --border-app: #e0e3e8;
  --border-panel: #e5e7eb;
  --border-input: #d1d5db;
  --border-input-focus: #6366f1;
  --border-dropzone: #d1d5db;
  --border-dropzone-active: #818cf8;
  --border-file-item: #e5e7eb;
  --border-file-item-hover: #c7d2fe;
  --border-checkbox: #d1d5db;
  --border-checkbox-hover: #a5b4fc;
  --border-warn: #fde68a;

  --text-primary: #1f2937;
  --text-secondary: #6b7280;
  --text-tertiary: #9ca3af;
  --text-inverse: #ffffff;
  --text-link: #4f46e5;

  --accent: #6366f1;
  --accent-hover: #4f46e5;
  --accent-light: #eef2ff;
  --accent-ring: rgba(99, 102, 241, 0.25);
  --danger: #ef4444;
  --danger-hover: #dc2626;
  --danger-light: #fef2f2;
  --success: #22c55e;
  --warn-text: #92400e;

  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.04);
  --shadow-md: 0 4px 12px rgba(0, 0, 0, 0.06);
  --radius: 6px;
  --radius-lg: 8px;
  --transition: 0.15s ease;
}

[data-theme="dark"] {
  --bg-app: #1a1b1e;
  --bg-toolbar: #25262b;
  --bg-panel: #25262b;
  --bg-input: #2c2e33;
  --bg-hover: #2c2e33;
  --bg-dropzone: #2c2e33;
  --bg-dropzone-hover: #31333a;
  --bg-file-item: #2c2e33;
  --bg-file-item-hover: #31333a;
  --bg-checkbox: #2c2e33;
  --bg-checkbox-hover: #31333a;
  --bg-warn: #3b2e0a;
  --bg-status-pending: #374151;
  --bg-status-queued: #312e81;
  --bg-status-processing: #1e3a5f;
  --bg-status-done: #14532d;
  --bg-status-error: #450a0a;

  --border-app: #2c2e33;
  --border-panel: #373a40;
  --border-input: #4b5563;
  --border-input-focus: #818cf8;
  --border-dropzone: #4b5563;
  --border-dropzone-active: #818cf8;
  --border-file-item: #373a40;
  --border-file-item-hover: #6366f1;
  --border-checkbox: #4b5563;
  --border-checkbox-hover: #818cf8;
  --border-warn: #78350f;

  --text-primary: #e5e7eb;
  --text-secondary: #9ca3af;
  --text-tertiary: #6b7280;
  --text-inverse: #1f2937;
  --text-link: #a5b4fc;

  --accent: #818cf8;
  --accent-hover: #6366f1;
  --accent-light: #1e1b4b;
  --accent-ring: rgba(129, 140, 248, 0.25);
  --danger: #f87171;
  --danger-hover: #ef4444;
  --danger-light: #450a0a;
  --success: #4ade80;
  --warn-text: #fbbf24;

  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.2);
  --shadow-md: 0 4px 12px rgba(0, 0, 0, 0.3);
}

/* ============================================
   Reset & Base
   ============================================ */
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html, body, #app {
  height: 100%;
  overflow: hidden;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
    "Helvetica Neue", Arial, "Noto Sans SC", sans-serif;
  font-size: 13px;
  background: var(--bg-app);
  color: var(--text-primary);
  -webkit-font-smoothing: antialiased;
}

/* ============================================
   App Shell — full window layout
   ============================================ */
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  width: 100vw;
  overflow: hidden;
}

/* ---- Toolbar ---- */
.app-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 44px;
  padding: 0 16px;
  background: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-app);
  flex-shrink: 0;
  -webkit-app-region: drag;
  user-select: none;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.toolbar-icon {
  font-size: 20px;
  color: var(--accent);
}

.toolbar-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.2px;
}

.toolbar-center {
  flex: 1;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 4px;
  -webkit-app-region: no-drag;
}

/* ---- Window control buttons (Windows 11 style) ---- */
.win-controls {
  display: flex;
  align-items: center;
  margin-left: 6px;
}

.win-btn {
  width: 46px;
  height: 32px;
  border: none;
  border-radius: 0;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.1s ease;
}

.win-btn:hover {
  background: var(--bg-hover);
}

.win-btn-close:hover {
  background: #e81123;
  color: #fff;
}

/* ---- Theme toggle button ---- */
.btn-icon {
  width: 32px;
  height: 32px;
  padding: 0;
  border: 1px solid var(--border-input);
  border-radius: var(--radius);
  background: var(--bg-input);
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 16px;
  transition: all var(--transition);
}

.btn-icon:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
  border-color: var(--border-input-focus);
}

.icon-sun,
.icon-moon {
  line-height: 1;
}

/* Show sun in light, moon in dark */
[data-theme="light"] .icon-moon { display: none; }
[data-theme="dark"]  .icon-sun  { display: none; }

/* ---- Main content area ---- */
.app-main {
  display: flex;
  flex: 1;
  overflow: hidden;
  gap: 0;
}

/* ---- Panels ---- */
.panel {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panel-input {
  width: 380px;
  min-width: 300px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-app);
  background: var(--bg-panel);
  padding: 16px;
  gap: 14px;
  overflow-y: auto;
}

.panel-list {
  flex: 1;
  background: var(--bg-panel);
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.panel-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.section-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

/* ---- Input groups ---- */
.input-group {
  display: flex;
  gap: 8px;
  align-items: center;
}

.input-group-col {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

input[type="text"],
textarea,
select {
  padding: 8px 12px;
  border: 1px solid var(--border-input);
  border-radius: var(--radius);
  font-size: 13px;
  font-family: inherit;
  outline: none;
  background: var(--bg-input);
  color: var(--text-primary);
  transition: border var(--transition), box-shadow var(--transition);
}

input[type="text"]:focus,
textarea:focus,
select:focus {
  border-color: var(--border-input-focus);
  box-shadow: 0 0 0 3px var(--accent-ring);
}

input[type="text"] {
  flex: 1;
}

textarea {
  resize: vertical;
  min-height: 56px;
}

/* ---- Buttons ---- */
.btn {
  padding: 8px 16px;
  border: none;
  border-radius: var(--radius);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all var(--transition);
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-family: inherit;
}

.btn-sm {
  padding: 4px 10px;
  font-size: 12px;
  border-radius: 4px;
}

.btn-outline {
  background: var(--bg-input);
  color: var(--text-primary);
  border: 1px solid var(--border-input);
}

.btn-outline:hover {
  background: var(--bg-hover);
  border-color: var(--border-input-focus);
  color: var(--accent);
}

.btn-outline:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--accent);
  color: #fff;
}

.btn-primary:hover {
  background: var(--accent-hover);
}

.btn-primary:active {
  transform: scale(0.98);
}

.btn-primary:disabled {
  opacity: 0.45;
  cursor: not-allowed;
  transform: none;
}

.btn-danger {
  background: transparent;
  color: var(--danger);
  border: 1px solid var(--border-input);
}

.btn-danger:hover {
  background: var(--danger-light);
  border-color: var(--danger);
}

/* ---- Dropzone ---- */
.dropzone {
  border: 2px dashed var(--border-dropzone);
  border-radius: var(--radius-lg);
  padding: 28px 16px;
  text-align: center;
  cursor: pointer;
  transition: all var(--transition);
  background: var(--bg-dropzone);
}

.dropzone:hover {
  border-color: var(--border-dropzone-active);
  background: var(--bg-dropzone-hover);
}

.dropzone.dragover {
  border-color: var(--accent);
  background: var(--accent-light);
  transform: scale(1.01);
}

.dropzone-icon {
  font-size: 32px;
  margin-bottom: 6px;
  transition: transform var(--transition);
}

.dropzone.dragover .dropzone-icon {
  transform: translateY(-3px) scale(1.08);
}

.dropzone-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.dropzone-sub {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-top: 4px;
}

/* ---- Checkbox row ---- */
.checkbox-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  background: var(--bg-checkbox);
  border-radius: var(--radius);
  border: 1px solid var(--border-checkbox);
  cursor: pointer;
  transition: all var(--transition);
  user-select: none;
}

.checkbox-row:hover {
  background: var(--bg-checkbox-hover);
  border-color: var(--border-checkbox-hover);
}

.checkbox-row input[type="checkbox"] {
  width: 16px;
  height: 16px;
  accent-color: var(--accent);
  cursor: pointer;
  flex-shrink: 0;
}

.checkbox-row span {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
}

.delete-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  padding-left: 4px;
}

/* ---- File list header ---- */
.file-list-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-panel);
  flex-shrink: 0;
}

.file-list-count {
  font-size: 12px;
  color: var(--text-secondary);
  font-weight: 600;
}

.file-list-actions {
  display: flex;
  gap: 8px;
}

/* ---- File list ---- */
.file-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.file-list:empty::after {
  content: "尚未添加文件";
  color: var(--text-tertiary);
  font-size: 13px;
  text-align: center;
  padding: 40px 0;
  display: block;
}

/* ---- File item ---- */
.file-item {
  display: flex;
  flex-direction: column;
  padding: 10px 14px;
  background: var(--bg-file-item);
  border-radius: var(--radius);
  border: 1px solid var(--border-file-item);
  animation: fadeUp 0.2s ease both;
  transition: all var(--transition);
  gap: 8px;
}

.file-item:hover {
  border-color: var(--border-file-item-hover);
  background: var(--bg-file-item-hover);
}

.file-item-top {
  display: flex;
  align-items: center;
  gap: 10px;
}

.file-icon {
  color: var(--accent);
  font-size: 16px;
  flex-shrink: 0;
}

.file-name {
  flex: 1;
  font-size: 13px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---- Status badges ---- */
.file-status {
  font-size: 11px;
  font-weight: 600;
  padding: 2px 10px;
  border-radius: 20px;
  flex-shrink: 0;
  white-space: nowrap;
}

.file-status.pending    { background: var(--bg-status-pending);    color: var(--text-secondary); }
.file-status.queued     { background: var(--bg-status-queued);     color: #818cf8; }
.file-status.processing { background: var(--bg-status-processing); color: #60a5fa; animation: pulse 1.5s infinite; }
.file-status.done       { background: var(--bg-status-done);       color: #4ade80; }
.file-status.error      { background: var(--bg-status-error);      color: #f87171; }

/* ---- Remove button ---- */
.file-remove {
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--radius);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  font-size: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all var(--transition);
  flex-shrink: 0;
}

.file-remove:hover {
  background: var(--danger-light);
  color: var(--danger);
}

/* ---- File item extra (conversion options) ---- */
.file-item-extra {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding-left: 26px;
}

.file-item-extra label {
  font-size: 12px;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  user-select: none;
}

.file-item-extra label input[type="checkbox"] {
  width: 14px;
  height: 14px;
  accent-color: var(--accent);
  cursor: pointer;
}

.file-item-extra select {
  padding: 4px 8px;
  border: 1px solid var(--border-input);
  border-radius: 4px;
  font-size: 12px;
  background: var(--bg-input);
  color: var(--text-primary);
  cursor: pointer;
  outline: none;
}

.file-item-extra select:focus {
  border-color: var(--border-input-focus);
}

.file-item-extra input[type="number"] {
  width: 68px;
  padding: 4px 8px;
  border: 1px solid var(--border-input);
  border-radius: 4px;
  font-size: 12px;
  text-align: center;
  outline: none;
  background: var(--bg-input);
  color: var(--text-primary);
}

.file-item-extra input[type="number"]:focus {
  border-color: var(--border-input-focus);
  box-shadow: 0 0 0 2px var(--accent-ring);
}

/* ---- Panel footer ---- */
.panel-footer {
  padding: 12px 16px;
  border-top: 1px solid var(--border-panel);
  flex-shrink: 0;
}

.btn-decrypt {
  width: 100%;
  padding: 12px;
  font-size: 14px;
  font-weight: 600;
}

/* ---- Toast notifications ---- */
.toast-container {
  position: fixed;
  top: 52px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 9999;
  display: flex;
  flex-direction: column;
  gap: 8px;
  pointer-events: none;
}

.toast {
  padding: 10px 22px;
  border-radius: var(--radius);
  font-size: 13px;
  font-weight: 500;
  color: #fff;
  box-shadow: var(--shadow-md);
  animation: toastIn 0.3s ease, toastOut 0.3s ease 2.5s forwards;
  pointer-events: auto;
  white-space: nowrap;
}

.toast.warn    { background: #f59e0b; }
.toast.info    { background: #6366f1; }
.toast.error   { background: #ef4444; }
.toast.success { background: #22c55e; }

/* ---- Animations ---- */
@keyframes fadeUp {
  from { opacity: 0; transform: translateY(8px); }
  to   { opacity: 1; transform: translateY(0); }
}

@keyframes toastIn {
  from { opacity: 0; transform: translateY(-10px) scale(0.95); }
  to   { opacity: 1; transform: translateY(0) scale(1); }
}

@keyframes toastOut {
  from { opacity: 1; transform: translateY(0) scale(1); }
  to   { opacity: 0; transform: translateY(-10px) scale(0.95); }
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50%      { opacity: 0.5; }
}

/* ---- Scrollbar ---- */
.file-list::-webkit-scrollbar,
.panel-input::-webkit-scrollbar {
  width: 5px;
}

.file-list::-webkit-scrollbar-track,
.panel-input::-webkit-scrollbar-track {
  background: transparent;
}

.file-list::-webkit-scrollbar-thumb,
.panel-input::-webkit-scrollbar-thumb {
  background: var(--border-input);
  border-radius: 10px;
}

.file-list::-webkit-scrollbar-thumb:hover,
.panel-input::-webkit-scrollbar-thumb:hover {
  background: var(--text-tertiary);
}
</style>