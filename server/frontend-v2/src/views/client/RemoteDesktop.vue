<template>
  <div class="remote-desktop">
    <div class="toolbar">
      <el-alert
        v-if="lastError"
        type="error"
        :closable="true"
        show-icon
        :title="lastError"
        @close="lastError = ''"
      />
      <el-alert
        v-if="!status.desktop_ready"
        type="warning"
        :closable="false"
        show-icon
        :title="statusHint"
      />
      <el-alert
        v-else
        type="info"
        :closable="false"
        show-icon
        :title="status.module_hint || '请先在「模块」面板加载 desktop，再连接。连接后数秒内应出帧。'"
      />
      <div class="controls">
        <el-button type="primary" :disabled="!canOpen || streaming" @click="startStream">连接 Desktop</el-button>
        <el-button type="danger" :disabled="!streaming" @click="stopStream">STOP</el-button>
        <el-button @click="refreshStatus">刷新状态</el-button>
        <el-tag v-if="status.desktop_busy" type="danger">busy</el-tag>
        <el-tag v-if="!status.yamux" type="warning">无 Yamux</el-tag>
        <el-tag v-if="streaming" type="success">streaming</el-tag>
        <el-tag v-if="canInput === false && streaming" type="warning">view-only</el-tag>
        <span class="meta">{{ status.transport || '?' }} · fps {{ fps }} · {{ frameInfo }}</span>
      </div>
    </div>
    <canvas
      ref="canvasRef"
      class="desk-canvas"
      :width="canvasW"
      :height="canvasH"
      @mousedown="onMouse"
      @mouseup="onMouse"
      @mousemove="onMove"
    />
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { ElMessage } from 'element-plus'
import api from '../../api/index'

const props = defineProps({
  clientId: { type: String, required: true },
  clientInfo: { type: Object, default: null },
})

const status = ref({
  desktop_ready: false,
  yamux: false,
  transport: '',
  desktop_busy: false,
  ws_unsupported: true,
  module_hint: '',
})
const streaming = ref(false)
const canInput = ref(null)
const lastError = ref('')
const fps = ref(5)
const maxW = ref(1280)
const canvasRef = ref(null)
const canvasW = ref(1280)
const canvasH = ref(720)
const frameInfo = ref('idle')
let ws = null
let ackTimer = null
let gotAck = false

const canOpen = computed(
  () => status.value.desktop_ready && !status.value.desktop_busy && !streaming.value
)

const statusHint = computed(() => {
  if (!status.value.yamux) {
    return '当前 Agent 无 Yamux（WebSocket-only 或未上线 TCP）。Desktop 仅支持 TCP 回连。'
  }
  return status.value.module_hint || 'Desktop 不可用'
})

async function refreshStatus() {
  try {
    const res = await api.get(`/api/desktop/${props.clientId}/status`)
    status.value = { ...status.value, ...res.data }
    if (res.data.error) {
      lastError.value = res.data.error
    }
  } catch (e) {
    const msg = e?.response?.data?.error || e?.message || 'status request failed'
    lastError.value = `状态查询失败: ${msg}`
    status.value = {
      desktop_ready: false,
      yamux: false,
      transport: props.clientInfo?.transport || '',
      desktop_busy: false,
      ws_unsupported: true,
      module_hint: msg,
    }
  }
}

function adminWsUrl() {
  const loc = window.location
  const proto = loc.protocol === 'https:' ? 'wss' : 'ws'
  const token = localStorage.getItem('cupcake_token') || ''
  if (!token) {
    lastError.value = '未登录：localStorage 无 cupcake_token'
  }
  return `${proto}://${loc.host}/api/desktop/${props.clientId}?fps=${fps.value}&quality=75&max_w=${maxW.value}&token=${encodeURIComponent(token)}`
}

function clearAckTimer() {
  if (ackTimer) {
    clearTimeout(ackTimer)
    ackTimer = null
  }
}

function startStream() {
  lastError.value = ''
  gotAck = false
  if (!canOpen.value) {
    const why = !status.value.desktop_ready
      ? statusHint.value
      : status.value.desktop_busy
        ? 'Desktop busy（其他操作员占用）'
        : '无法连接'
    ElMessage.warning(why)
    lastError.value = why
    return
  }
  const url = adminWsUrl()
  frameInfo.value = 'connecting…'
  try {
    ws = new WebSocket(url)
  } catch (e) {
    lastError.value = `WebSocket 创建失败: ${e}`
    ElMessage.error(lastError.value)
    return
  }
  ws.binaryType = 'arraybuffer'
  streaming.value = true

  ws.onopen = () => {
    frameInfo.value = 'ws open, waiting HELLO_ACK…'
    clearAckTimer()
    // If agent never answers, surface error (do not hang forever)
    ackTimer = setTimeout(() => {
      if (!gotAck && streaming.value) {
        lastError.value =
          '超时：未收到 HELLO_ACK/ERROR。请确认：1) TCP Yamux Agent；2) 模块面板已加载 desktop；3) 服务端/Agent 均为新版本。'
        ElMessage.error(lastError.value)
        stopStream()
      }
    }, 9000)
  }

  ws.onmessage = (ev) => {
    if (typeof ev.data === 'string') {
      try {
        const j = JSON.parse(ev.data)
        const code = j.code || 'error'
        const msg = j.msg || j.error || code
        lastError.value = `[${code}] ${msg}`
        ElMessage.error(lastError.value)
        frameInfo.value = lastError.value
        if (code) stopStream()
      } catch (_) {
        lastError.value = String(ev.data)
        ElMessage.error(lastError.value)
      }
      return
    }
    handleBinary(new Uint8Array(ev.data))
  }

  ws.onerror = () => {
    // Browser does not expose HTTP status on WS error
    lastError.value =
      'WebSocket 错误（常见：401 token、404 agent 离线、403 IP、或 upgrade 失败）。请看服务端 [desktop]/[Security] 日志。'
    ElMessage.error(lastError.value)
    frameInfo.value = 'ws error'
  }

  ws.onclose = (ev) => {
    clearAckTimer()
    streaming.value = false
    const reason = ev?.reason || ''
    frameInfo.value = `closed code=${ev?.code || '?'} ${reason}`
    if (!gotAck && !lastError.value) {
      lastError.value = `连接关闭 code=${ev?.code} ${reason || '(无 reason)'} — 若为 1006 多为鉴权失败或服务端拒绝 upgrade`
      ElMessage.error(lastError.value)
    }
    refreshStatus()
  }
}

function stopStream() {
  clearAckTimer()
  if (ws && ws.readyState === WebSocket.OPEN) {
    try {
      ws.send(JSON.stringify({ type: 'stop' }))
    } catch (_) {}
    try {
      ws.close()
    } catch (_) {}
  }
  ws = null
  streaming.value = false
  if (frameInfo.value === 'connecting…' || frameInfo.value.startsWith('ws open')) {
    frameInfo.value = 'stopped'
  }
  refreshStatus()
}

function handleBinary(buf) {
  if (buf.length < 12) return
  if (buf[0] !== 0x43 || buf[1] !== 0x50 || buf[2] !== 0x58 || buf[3] !== 0x44) {
    lastError.value = '收到非 CPXD 二进制帧'
    return
  }
  const msgType = buf[5]
  const plen = buf[8] | (buf[9] << 8) | (buf[10] << 16) | (buf[11] << 24)
  if (plen < 0 || 12 + plen > buf.length) return
  const payload = buf.subarray(12, 12 + plen)

  if (msgType === 0x02) {
    gotAck = true
    clearAckTimer()
    try {
      const j = JSON.parse(new TextDecoder().decode(payload))
      canInput.value = !!j.can_input
      if (j.w) canvasW.value = j.w
      if (j.h) canvasH.value = j.h
      frameInfo.value = `ack ${j.w}x${j.h} encode=${j.encode}`
      ElMessage.success('Desktop 已握手 (HELLO_ACK)')
    } catch (_) {
      frameInfo.value = 'HELLO_ACK parse error'
    }
  } else if (msgType === 0x03 && payload.length > 16) {
    gotAck = true
    clearAckTimer()
    const w = payload[0] | (payload[1] << 8)
    const h = payload[2] | (payload[3] << 8)
    const jpeg = payload.subarray(16)
    canvasW.value = w
    canvasH.value = h
    frameInfo.value = `frame ${w}x${h} ${jpeg.length}B`
    drawJpeg(jpeg)
  } else if (msgType === 0x08) {
    gotAck = true
    clearAckTimer()
    let code = 'agent_error'
    let msg = new TextDecoder().decode(payload)
    try {
      const j = JSON.parse(msg)
      code = j.code || code
      msg = j.msg || msg
    } catch (_) {}
    lastError.value = `[${code}] ${msg}`
    frameInfo.value = lastError.value
    ElMessage.error(lastError.value)
    stopStream()
  }
}

function drawJpeg(jpeg) {
  const blob = new Blob([jpeg], { type: 'image/jpeg' })
  const url = URL.createObjectURL(blob)
  const img = new Image()
  img.onload = () => {
    const c = canvasRef.value
    if (!c) return
    const ctx = c.getContext('2d')
    ctx.drawImage(img, 0, 0, c.width, c.height)
    URL.revokeObjectURL(url)
  }
  img.onerror = () => {
    URL.revokeObjectURL(url)
  }
  img.src = url
}

function sendInput(x, y, button, down) {
  if (!ws || ws.readyState !== WebSocket.OPEN) return
  const body = new Uint8Array(7)
  body[0] = 2
  body[1] = x & 0xff
  body[2] = (x >> 8) & 0xff
  body[3] = y & 0xff
  body[4] = (y >> 8) & 0xff
  body[5] = button
  body[6] = down
  const msg = new Uint8Array(12 + body.length)
  msg[0] = 0x43
  msg[1] = 0x50
  msg[2] = 0x58
  msg[3] = 0x44
  msg[4] = 1
  msg[5] = 0x04
  msg[8] = body.length
  msg.set(body, 12)
  ws.send(msg.buffer)
}

function canvasCoords(ev) {
  const c = canvasRef.value
  const r = c.getBoundingClientRect()
  const x = Math.floor(((ev.clientX - r.left) / r.width) * c.width)
  const y = Math.floor(((ev.clientY - r.top) / r.height) * c.height)
  return [x, y]
}

function onMouse(ev) {
  if (!streaming.value) return
  const [x, y] = canvasCoords(ev)
  sendInput(x, y, 1, ev.type === 'mousedown' ? 1 : 0)
}

function onMove(ev) {
  if (!streaming.value || ev.buttons === 0) return
  const [x, y] = canvasCoords(ev)
  sendInput(x, y, 1, 1)
}

onMounted(refreshStatus)
onUnmounted(stopStream)
watch(() => props.clientId, refreshStatus)
</script>

<style scoped>
.remote-desktop {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  padding: 8px;
}
.controls {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-top: 8px;
}
.meta {
  color: #888;
  font-size: 12px;
}
.desk-canvas {
  flex: 1;
  width: 100%;
  max-height: calc(100vh - 220px);
  background: #111;
  border: 1px solid #333;
  cursor: crosshair;
}
</style>
