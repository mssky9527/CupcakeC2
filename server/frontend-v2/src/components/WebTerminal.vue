

<script setup>
import { onMounted, onUnmounted, ref, nextTick } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'

// Props: accept socket from parent
const props = defineProps({
    socket: Object, 
    clientId: String,
    allowPTY: {
        type: Boolean,
        default: false
    }
})

const terminalContainer = ref(null)
let term = null
let fitAddon = null
let ptySocket = null // Dedicated socket for PTY
let ptyTextBuffer = ''
let ptyMode = 'unknown' // 'unknown' | 'yamux' | 'fallback'
let fallbackInputBuffer = ''
let fallbackHistoryIndex = -1
let fallbackBannerShown = false
let fallbackPrompt = '> '
let fallbackOutputBuffer = ''
const fallbackHistory = []
const fallbackDoneToken = '__CUPCAKE_DONE__'
let promptVisible = false
let lastPromptAt = 0

const historyBuffer = []
const pendingBuffer = []
const storageKey = () => `terminal_history_${props.clientId}`

const persistHistory = () => {
  try {
    localStorage.setItem(storageKey(), JSON.stringify(historyBuffer.slice(-1000)))
  } catch (_) {
    // ignore storage errors
  }
}

const restoreHistory = () => {
  try {
    const raw = localStorage.getItem(storageKey())
    if (raw) {
      const items = JSON.parse(raw)
      if (Array.isArray(items)) {
        items.forEach((line) => term && term.write(line))
      }
    }
  } catch (_) {
    // ignore parse errors
  }
}

const appendOutput = (content) => {
  if (!content) return
  const text = String(content).replace(/\r?\n/g, '\r\n')
  if (!term) {
    pendingBuffer.push(text)
    return
  }
  term.write(text)
  historyBuffer.push(text)
  persistHistory()
}

const flushPending = () => {
  if (!term || pendingBuffer.length === 0) return
  pendingBuffer.splice(0).forEach(chunk => term.write(chunk))
}

const handlePtyJsonMessage = (jsonStr) => {
  try {
    const msg = JSON.parse(jsonStr)
    if (msg && msg.type === 'PTY_MODE') {
      if (msg.content === 'fallback') {
        ptyMode = 'fallback'
        if (!fallbackBannerShown) {
          term.writeln('\x1b[33m[Line Mode] 回车发送，↑/↓ 历史，Ctrl+L 清屏，Ctrl+U 清空当前行\x1b[0m')
          fallbackBannerShown = true
        }
      }
      return
    }
    if (msg && msg.type === 'PTY_DONE') {
      if (ptyMode !== 'yamux') {
        ptyMode = 'fallback'
        showPrompt()
      }
      return
    }
    if (msg && msg.type === 'TERM') {
      ptyMode = 'fallback'
      if (msg.content !== undefined && msg.content !== null) {
        writeFallbackOutput(String(msg.content))
      }
      return
    }
    if (msg && msg.type === 'JSON_DATA') {
      ptyMode = 'fallback'
      // JSON data received: silently filter from terminal display
      return
    }
    if (msg && msg.content !== undefined && msg.content !== null) {
      ptyMode = 'fallback'
      writeFallbackOutput(String(msg.content))
      return
    }
  } catch (_) {
    // fall through to raw output
  }
  term.write(jsonStr)
}

const consumePtyText = (chunk) => {
  if (!chunk) return
  ptyTextBuffer += String(chunk)
  const buffer = ptyTextBuffer
  let i = 0
  let start = -1
  let depth = 0
  let inString = false
  let escape = false
  let lastProcessed = 0

  while (i < buffer.length) {
    const ch = buffer[i]
    if (start === -1) {
      if (ch === '{' || ch === '[') {
        start = i
        depth = 0
      } else {
        i++
        continue
      }
    }

    if (inString) {
      if (escape) {
        escape = false
      } else if (ch === '\\\\') {
        escape = true
      } else if (ch === '"') {
        inString = false
      }
      i++
      continue
    }

    if (ch === '"') {
      inString = true
      i++
      continue
    }

    if (ch === '{' || ch === '[') {
      depth++
    } else if (ch === '}' || ch === ']') {
      depth--
      if (depth === 0 && start !== -1) {
        const jsonStr = buffer.slice(start, i + 1)
        handlePtyJsonMessage(jsonStr)
        lastProcessed = i + 1
        start = -1
      }
    }
    i++
  }

  if (start !== -1 || depth > 0) {
    ptyTextBuffer = buffer.slice(start)
    return
  }

  const remaining = buffer.slice(lastProcessed)
  if (remaining) {
    term.write(remaining)
  }
  ptyTextBuffer = ''
}

const rememberFallbackCommand = (cmd) => {
  const trimmed = String(cmd || '').trim()
  if (!trimmed) {
    fallbackHistoryIndex = -1
    return
  }
  if (fallbackHistory.length === 0 || fallbackHistory[fallbackHistory.length - 1] !== trimmed) {
    fallbackHistory.push(trimmed)
  }
  if (fallbackHistory.length > 100) {
    fallbackHistory.shift()
  }
  fallbackHistoryIndex = -1
}

const historyUp = () => {
  if (fallbackHistory.length === 0) return
  if (fallbackHistoryIndex === -1) {
    fallbackHistoryIndex = fallbackHistory.length - 1
  } else if (fallbackHistoryIndex > 0) {
    fallbackHistoryIndex -= 1
  }
  replaceFallbackLine(fallbackHistory[fallbackHistoryIndex])
}

const historyDown = () => {
  if (fallbackHistoryIndex === -1) return
  if (fallbackHistoryIndex < fallbackHistory.length - 1) {
    fallbackHistoryIndex += 1
    replaceFallbackLine(fallbackHistory[fallbackHistoryIndex])
    return
  }
  fallbackHistoryIndex = -1
  replaceFallbackLine('')
}

const showPrompt = () => {
  if (fallbackInputBuffer.length > 0) return
  const now = Date.now()
  if (promptVisible && now - lastPromptAt < 200) return
  term.write(fallbackPrompt)
  promptVisible = true
  lastPromptAt = now
}

const writeFallbackOutput = (content) => {
  if (!content) return
  promptVisible = false
  let text = String(content)

  // 1. 无缝剥离控制标记，无论是否换行
  if (fallbackDoneToken) {
    const regex = new RegExp(fallbackDoneToken, 'g')
    text = text.replace(regex, '')
  }

  // 2. 剥离无用的回音残留
  text = text.replace(/@echo off\r?\n?/gi, '')
  text = text.replace(/echo off\r?\n?/gi, '')
  text = text.replace(/ECHO is off\.\r?\n?/gi, '')

  // 3. 释放到终端（全量渲染残余的 Prompt 提示符等，不再需要换行才执行）
  if (text) {
    term.write(text)
  }
}

let fallbackCursorIndex = 0

const replaceFallbackLine = (text) => {
  clearFallbackLine()
  fallbackInputBuffer = text
  fallbackCursorIndex = text.length
  if (text) {
    term.write(text)
  }
}

const clearFallbackLine = () => {
  if (!fallbackInputBuffer) return
  // 将光标移到最左，再清除整行
  term.write('\r\x1b[K')
  // 恢复 Prompt
  term.write(fallbackPrompt)
  fallbackInputBuffer = ''
  fallbackCursorIndex = 0
}

const handleFallbackInput = (data) => {
  if (!data) return
  const text = String(data)

  if (text === '\x1b[A') { // UP
    historyUp()
    return
  }
  if (text === '\x1b[B') { // DOWN
    historyDown()
    return
  }
  if (text === '\x1b[C') { // RIGHT
    if (fallbackCursorIndex < fallbackInputBuffer.length) {
      fallbackCursorIndex++
      term.write('\x1b[C')
    }
    return
  }
  if (text === '\x1b[D') { // LEFT
    if (fallbackCursorIndex > 0) {
      fallbackCursorIndex--
      term.write('\x1b[D')
    }
    return
  }
  if (text.startsWith('\x1b')) {
    return
  }

  for (const ch of text) {
    if (ch === '\x15') { // Ctrl+U
      clearFallbackLine()
      continue
    }
    if (ch === '\x0c') { // Ctrl+L
      term.clear()
      term.write('\r' + fallbackPrompt + fallbackInputBuffer)
      // 恢复光标位置
      if (fallbackInputBuffer.length > fallbackCursorIndex) {
         term.write('\x1b[' + (fallbackInputBuffer.length - fallbackCursorIndex) + 'D')
      }
      continue
    }
    if (ch === '\x03') { // Ctrl+C
      replaceFallbackLine('')
      term.write('^C\r\n' + fallbackPrompt)
      continue
    }
    if (ch === '\r' || ch === '\n') {
      term.write('\r\n')
      if (ptySocket && ptySocket.readyState === WebSocket.OPEN) {
        rememberFallbackCommand(fallbackInputBuffer)
        ptySocket.send(fallbackInputBuffer + '\r\n')
      }
      fallbackInputBuffer = ''
      fallbackCursorIndex = 0
      continue
    }
    if (ch === '\x7f' || ch === '\b') {
      if (fallbackCursorIndex > 0) {
        // 删除光标左侧的一个字符
        const before = fallbackInputBuffer.slice(0, fallbackCursorIndex - 1)
        const after = fallbackInputBuffer.slice(fallbackCursorIndex)
        fallbackInputBuffer = before + after
        fallbackCursorIndex--
        
        // 重绘光标及之后的内容
        term.write('\b\x1b[K' + after)
        // 光标向左退回
        if (after.length > 0) {
          term.write('\x1b[' + after.length + 'D')
        }
      }
      if (fallbackHistoryIndex !== -1) {
        fallbackHistoryIndex = -1
      }
      continue
    }

    // 正常输入：插入字符
    const before = fallbackInputBuffer.slice(0, fallbackCursorIndex)
    const after = fallbackInputBuffer.slice(fallbackCursorIndex)
    fallbackInputBuffer = before + ch + after
    fallbackCursorIndex++

    // 向后重绘字符
    term.write(ch + after)
    if (after.length > 0) {
      term.write('\x1b[' + after.length + 'D')
    }

    promptVisible = false
    if (fallbackHistoryIndex !== -1) {
      fallbackHistoryIndex = -1
    }
  }
}
// 🛡️ 1. PTY Socket Handler
const initPTY = () => {
  const token = localStorage.getItem('cupcake_token')
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws'
  const wsUrl = `${protocol}://${window.location.host}/api/pty/${props.clientId}?token=${encodeURIComponent(token)}`
  
  ptySocket = new WebSocket(wsUrl)
  ptySocket.binaryType = 'arraybuffer' 

  ptySocket.onopen = () => {
    term.writeln('\x1b[1;32m[+] Interactive PTY Connected.\x1b[0m\r\n')
    term.focus()
  }

  ptySocket.onmessage = (event) => {
    if (event.data instanceof ArrayBuffer) {
      const bytes = new Uint8Array(event.data)
      const textPreview = new TextDecoder('utf-8').decode(bytes)

      const fallbackMagic = '{"type": "PTY_MODE", "content": "fallback"}'
      if (textPreview.includes(fallbackMagic)) {
        ptyMode = 'fallback'
        if (!fallbackBannerShown) {
          term.writeln('\x1b[33m[Line Mode] 回车发送，↑/↓ 历史，Ctrl+L 清屏，Ctrl+U 清空当前行\x1b[0m')
          fallbackBannerShown = true
        }
        const cleanText = textPreview.replace(fallbackMagic, '')
        writeFallbackOutput(cleanText)
        return
      }

      if (ptyMode === 'fallback') {
        writeFallbackOutput(textPreview)
        return
      }

      // Raw bytes from Yamux (Traditional PTY)
      ptyMode = 'yamux'
      term.write(bytes)
      return
    }

    if (event.data instanceof Blob) {
      event.data.text().then((text) => consumePtyText(text))
      return
    }
    // Potential JSON wrapped data (Fallback PTY)
    consumePtyText(event.data)
  }

  ptySocket.onclose = () => {
    term.writeln('\r\n\x1b[1;31m[!] PTY Session Closed.\x1b[0m')
  }

  ptySocket.onerror = () => {
    term.writeln('\r\n\x1b[1;31m[!] PTY Connection Error.\x1b[0m')
  }

  // Bind Input
  term.onData((data) => {
    if (ptySocket && ptySocket.readyState === WebSocket.OPEN) {
        if (ptyMode === 'fallback') {
          handleFallbackInput(data)
          return
        }
        ptySocket.send(data)
    }
  })
}

const initTerminal = () => {
  term = new Terminal({
    cursorBlink: true,
    fontSize: 14,
    fontFamily: '"Consolas", "Monaco", monospace',
    fontWeight: 'normal', // Standard for PTY
    allowTransparency: true,
    scrollback: 5000, 
    theme: {
      background: '#000000', // Pure Black
      foreground: '#d4d4d4',
      cursor: '#007acc',
    }
  })
  
  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  term.open(terminalContainer.value)
  fitAddon.fit()
  
  if (props.allowPTY) {
    initPTY()
  } else {
    // Legacy Mode (Restored Buffer)
    restoreHistory()
    if (!localStorage.getItem(storageKey())) {
      term.writeln('\x1b[1;37m[System] Terminal Ready.\x1b[0m')
    }
  }

  flushPending()
}

const handleSocketMessage = (event) => {
  if (props.allowPTY) return
  if (!event || !event.data) return

  let packetType = 'TERM'
  let content = event.data

  if (typeof content === 'string') {
    try {
      const parsed = JSON.parse(content)
      if (parsed && parsed.type) {
        packetType = parsed.type
        content = parsed.content ?? ''
      }
    } catch (_) {
      // not JSON, treat as plain text
    }
  }

  if (packetType !== 'TERM') return

  if (content instanceof ArrayBuffer) {
    appendOutput(new TextDecoder().decode(new Uint8Array(content)))
  } else {
    appendOutput(content)
  }
}

const clearHistory = () => {
  historyBuffer.length = 0
  try {
    localStorage.removeItem(storageKey())
  } catch (_) {
    // ignore
  }
}

// Expose handler for parent to bind (Legacy mode only)
defineExpose({ handleSocketMessage, clearHistory })

onMounted(() => {
  initTerminal()
  window.addEventListener('resize', () => fitAddon && fitAddon.fit())
})

onUnmounted(() => {
  if (ptySocket) ptySocket.close()
  if (term) term.dispose()
})
</script>

<template>
  <div class="terminal-wrapper">
    <div ref="terminalContainer" class="terminal-container"></div>
  </div>
</template>

<style scoped>
.terminal-wrapper {
  /* Absolute Fill */
  width: 100%;
  height: 100%; 
  
  /* Black Theme Override */
  background-color: #000000 !important;
  border-radius: 0;
  padding: 10px !important;
  box-sizing: border-box;
}

.terminal-container {
  width: 100%;
  height: 100%;
}

/* Force Text White */
:deep(.xterm-rows) {
  color: #ffffff !important;
}

:deep(.xterm-viewport), :deep(.xterm-screen), :deep(.xterm-rows) {
  padding: 0 !important;
  margin: 0 !important;
}
</style>

