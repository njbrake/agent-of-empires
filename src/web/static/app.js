// Single-page dashboard: sidebar session list + PTY-relayed terminal viewer

let sessions = [];
let activeSessionId = null;
let ws = null;
let term = null;
let fitAddon = null;

// DOM refs
const sessionList = document.getElementById('session-list');
const sidebarActions = document.getElementById('sidebar-actions');
const contentHeader = document.getElementById('content-header');
const terminalArea = document.getElementById('terminal-area');
const emptyState = document.getElementById('empty-state');
const selectedTitle = document.getElementById('selected-title');
const selectedMeta = document.getElementById('selected-meta');
const selectedStatus = document.getElementById('selected-status');
const statusEl = document.getElementById('connection-status');
const mobileBack = document.getElementById('mobile-back');

// Fetch sessions from API
async function fetchSessions() {
  try {
    const res = await fetch('/api/sessions');
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return await res.json();
  } catch (e) {
    return null;
  }
}

// Render sidebar session list
function renderSessionList() {
  if (!sessions || sessions.length === 0) {
    sessionList.innerHTML = '<div id="no-sessions">No sessions found.<br>Create sessions via CLI.</div>';
    sidebarActions.style.display = 'none';
    return;
  }

  sessionList.innerHTML = sessions.map(s => `
    <div class="session-item ${s.id === activeSessionId ? 'active' : ''}"
         data-id="${s.id}" onclick="selectSession('${s.id}')">
      <div class="title">
        <span class="status-dot status-${s.status}"></span>
        ${escapeHtml(s.title)}
      </div>
      <div class="meta">
        <span>${escapeHtml(s.tool)}</span>
        ${s.branch ? `<span>${escapeHtml(s.branch)}</span>` : ''}
      </div>
    </div>
  `).join('');

  updateSidebarActions();
}

// Update action buttons for selected session
function updateSidebarActions() {
  const session = sessions.find(s => s.id === activeSessionId);
  if (!session) {
    sidebarActions.style.display = 'none';
    return;
  }

  const buttons = [];
  if (session.status !== 'Stopped') {
    buttons.push(`<button class="btn btn-danger" onclick="stopSession('${session.id}')">Stop</button>`);
  }
  if (session.status === 'Stopped' || session.status === 'Error') {
    buttons.push(`<button class="btn btn-primary" onclick="restartSession('${session.id}')">Restart</button>`);
  }

  sidebarActions.innerHTML = buttons.join('');
  sidebarActions.style.display = buttons.length ? 'flex' : 'none';
}

// Select and connect to a session
function selectSession(id) {
  if (activeSessionId === id) return;
  activeSessionId = id;

  const session = sessions.find(s => s.id === id);
  if (!session) return;

  // Update header
  selectedTitle.textContent = session.title;
  selectedMeta.textContent = [session.tool, session.branch, session.is_sandboxed ? 'sandboxed' : '']
    .filter(Boolean).join(' \u00b7 ');
  selectedStatus.innerHTML = `<span class="status-dot status-${session.status}"></span>${session.status}`;

  contentHeader.style.display = '';
  emptyState.style.display = 'none';
  terminalArea.style.display = '';

  // Re-highlight sidebar
  renderSessionList();

  // Mobile: hide sidebar, show content
  if (window.innerWidth <= 700) {
    document.getElementById('sidebar').classList.add('hidden');
    document.getElementById('content').classList.remove('hidden');
  }

  // Connect terminal via PTY relay
  connectTerminal(id);
}

// Connect WebSocket to session terminal (PTY relay)
function connectTerminal(sessionId) {
  // Clean up previous connection
  if (ws) {
    ws.close();
    ws = null;
  }
  if (term) {
    term.dispose();
    term = null;
  }

  // Clear terminal area
  terminalArea.innerHTML = '';

  // Create terminal
  term = new Terminal({
    cursorBlink: true,
    fontSize: 14,
    fontFamily: "'SF Mono', 'Fira Code', 'Cascadia Code', Menlo, monospace",
    theme: {
      background: '#0d1117',
      foreground: '#c9d1d9',
      cursor: '#58a6ff',
      selectionBackground: '#264f78',
    },
  });

  fitAddon = new FitAddon.FitAddon();
  term.loadAddon(fitAddon);
  term.open(terminalArea);

  requestAnimationFrame(() => {
    fitAddon.fit();
  });

  // WebSocket -- use binary mode for raw PTY data
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  ws = new WebSocket(`${proto}//${location.host}/sessions/${sessionId}/ws`);
  ws.binaryType = 'arraybuffer';

  ws.onopen = () => {
    term.focus();
    // Send initial terminal size as a JSON control message
    const dims = fitAddon.proposeDimensions();
    if (dims) {
      ws.send(JSON.stringify({ type: 'resize', cols: dims.cols, rows: dims.rows }));
    }
  };

  ws.onmessage = (event) => {
    if (event.data instanceof ArrayBuffer) {
      // Binary frame: raw PTY output -> write directly to xterm.js
      term.write(new Uint8Array(event.data));
    } else {
      // Text frame: shouldn't happen in normal flow, but handle gracefully
      term.write(event.data);
    }
  };

  ws.onclose = () => {
    term.write('\r\n\x1b[33m[Connection closed]\x1b[0m\r\n');
  };

  ws.onerror = () => {
    term.write('\r\n\x1b[31m[WebSocket error]\x1b[0m\r\n');
  };

  // Relay keystrokes: xterm.js sends raw escape sequences -> binary to PTY stdin
  term.onData((data) => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      // Send as binary for direct PTY write
      const encoder = new TextEncoder();
      ws.send(encoder.encode(data));
    }
  });

  // Handle resize
  term.onResize(({ cols, rows }) => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'resize', cols, rows }));
    }
  });
}

// Handle window resize
window.addEventListener('resize', () => {
  if (fitAddon && term) {
    fitAddon.fit();
  }
});

// Mobile back button
mobileBack.addEventListener('click', () => {
  document.getElementById('sidebar').classList.remove('hidden');
  document.getElementById('content').classList.add('hidden');
});

// Session actions
async function stopSession(id) {
  await fetch(`/api/sessions/${id}/stop`, { method: 'POST' });
  refresh();
}

async function restartSession(id) {
  await fetch(`/api/sessions/${id}/restart`, { method: 'POST' });
  refresh();
}

// Refresh loop
async function refresh() {
  const data = await fetchSessions();
  if (data !== null) {
    sessions = data;
    renderSessionList();
    statusEl.textContent = `${sessions.length} session${sessions.length !== 1 ? 's' : ''}`;

    // Update header status if a session is selected
    if (activeSessionId) {
      const session = sessions.find(s => s.id === activeSessionId);
      if (session) {
        selectedStatus.innerHTML = `<span class="status-dot status-${session.status}"></span>${session.status}`;
      }
    }
  } else {
    statusEl.textContent = 'Connection error';
  }
}

function escapeHtml(str) {
  if (!str) return '';
  const d = document.createElement('div');
  d.textContent = str;
  return d.innerHTML;
}

// Register service worker for PWA install support
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('/static/sw.js');
}

// Initial load
refresh();
setInterval(refresh, 3000);
