<template>
  <div class="remote-desktop">
    <el-alert
      v-if="lastError"
      type="error"
      :closable="true"
      show-icon
      :title="lastError"
      @close="lastError = ''"
      style="margin-bottom: 12px"
    />
    <el-alert
      v-if="!status.desktop_ready"
      type="warning"
      :closable="false"
      show-icon
      :title="statusHint"
      style="margin-bottom: 12px"
    />
    <el-alert
      v-else
      type="info"
      :closable="false"
      show-icon
      :title="status.module_hint || '① 模块面板加载 desktop ② 启动 RDP 转发 ③ mstsc 连 C2 监听端口（Agent → 目标 3389）。'"
      style="margin-bottom: 12px"
    />

    <el-card shadow="never" class="rdp-card">
      <template #header>
        <div class="card-header">
          <span>远程桌面 · RDP 模块 (3389)</span>
          <div class="header-tags">
            <el-tag v-if="status.rdp_active" type="success">转发中</el-tag>
            <el-tag v-else type="info">未启动</el-tag>
            <el-tag v-if="!status.yamux" type="warning">无 Yamux</el-tag>
            <el-tag type="info">{{ status.transport || '?' }}</el-tag>
          </div>
        </div>
      </template>

      <el-form label-position="top" class="rdp-form" @submit.prevent>
        <el-row :gutter="16">
          <el-col :xs="24" :sm="8">
            <el-form-item label="Agent 侧目标主机">
              <el-input
                v-model="form.targetHost"
                :disabled="status.rdp_active"
                placeholder="127.0.0.1"
              />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="Agent 侧目标端口">
              <el-input-number
                v-model="form.targetPort"
                :min="1"
                :max="65535"
                :disabled="status.rdp_active"
                controls-position="right"
                style="width: 100%"
              />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="C2 监听端口（0=自动分配）">
              <el-input-number
                v-model="form.listenPort"
                :min="0"
                :max="65535"
                :disabled="status.rdp_active"
                controls-position="right"
                style="width: 100%"
              />
            </el-form-item>
          </el-col>
        </el-row>

        <div class="controls">
          <el-button
            type="primary"
            :disabled="!canStart"
            :loading="starting"
            @click="startRdp"
          >
            启动 RDP 转发
          </el-button>
          <el-button
            type="danger"
            :disabled="!status.rdp_active"
            :loading="stopping"
            @click="stopRdp"
          >
            停止
          </el-button>
          <el-button @click="refreshStatus">刷新状态</el-button>
        </div>
      </el-form>

      <el-divider v-if="status.rdp_active" />

      <div v-if="status.rdp_active" class="connect-box">
        <h4>连接方式</h4>
        <p class="hint">
          在操作机上使用远程桌面客户端连接 <strong>C2 服务器</strong> 的监听端口；
          流量经 Agent 转发到 <code>{{ status.target_host }}:{{ status.target_port }}</code>。
        </p>
        <el-descriptions :column="1" border size="small">
          <el-descriptions-item label="C2 监听">
            <code>0.0.0.0:{{ status.listen_port }}</code>
          </el-descriptions-item>
          <el-descriptions-item label="Agent 目标">
            <code>{{ status.target_host }}:{{ status.target_port }}</code>
          </el-descriptions-item>
          <el-descriptions-item label="mstsc 命令">
            <div class="cmd-row">
              <code>{{ mstscCmd }}</code>
              <el-button size="small" @click="copyText(mstscCmd)">复制</el-button>
            </div>
          </el-descriptions-item>
          <el-descriptions-item label="连接地址">
            <div class="cmd-row">
              <code>{{ connectAddr }}</code>
              <el-button size="small" @click="copyText(connectAddr)">复制</el-button>
            </div>
          </el-descriptions-item>
        </el-descriptions>
        <el-alert
          type="warning"
          :closable="false"
          show-icon
          style="margin-top: 12px"
          title="须已加载 L2 模块 desktop。目标需开启远程桌面（3389）。默认 127.0.0.1:3389；内网其它主机可改「目标主机」。若连接失败，优先检查模块是否 Loaded。"
        />
      </div>
    </el-card>
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
  rdp_active: false,
  mode: 'rdp',
  module_hint: '',
  listen_port: 0,
  target_host: '127.0.0.1',
  target_port: 3389,
})
const lastError = ref('')
const starting = ref(false)
const stopping = ref(false)
const form = ref({
  targetHost: '127.0.0.1',
  targetPort: 3389,
  listenPort: 0,
})

const canStart = computed(
  () => status.value.desktop_ready && !status.value.rdp_active && !starting.value
)

const statusHint = computed(() => {
  if (!status.value.yamux) {
    return '当前 Agent 无 Yamux（WebSocket-only 或未上线 TCP）。RDP 转发仅支持 TCP 回连。'
  }
  return status.value.module_hint || 'Desktop 不可用'
})

/** Prefer page host; operator may replace with public C2 IP. */
const c2Host = computed(() => {
  const h = window.location.hostname
  if (!h || h === 'localhost' || h === '127.0.0.1') {
    return '<C2主机IP>'
  }
  return h
})

const connectAddr = computed(() => {
  const port = status.value.listen_port
  if (!port) return ''
  return `${c2Host.value}:${port}`
})

const mstscCmd = computed(() => {
  if (!status.value.listen_port) return ''
  return `mstsc /v:${connectAddr.value}`
})

async function refreshStatus() {
  try {
    const res = await api.get(`/api/desktop/${props.clientId}/status`)
    status.value = { ...status.value, ...res.data }
    if (res.data.error) {
      lastError.value = res.data.error
    }
    // Sync form from active session
    if (res.data.rdp_active) {
      if (res.data.target_host) form.value.targetHost = res.data.target_host
      if (res.data.target_port) form.value.targetPort = res.data.target_port
      if (res.data.listen_port) form.value.listenPort = res.data.listen_port
    }
  } catch (e) {
    const msg = e?.response?.data?.error || e?.message || 'status request failed'
    lastError.value = `状态查询失败: ${msg}`
    status.value = {
      desktop_ready: false,
      yamux: false,
      transport: props.clientInfo?.transport || '',
      desktop_busy: false,
      rdp_active: false,
      mode: 'rdp',
      module_hint: msg,
    }
  }
}

async function startRdp() {
  lastError.value = ''
  if (!canStart.value) {
    ElMessage.warning(statusHint.value)
    return
  }
  starting.value = true
  try {
    const res = await api.post(`/api/desktop/${props.clientId}/start`, {
      target_host: form.value.targetHost || '127.0.0.1',
      target_port: form.value.targetPort || 3389,
      listen_port: form.value.listenPort || 0,
    })
    ElMessage.success(res.data?.msg || 'RDP 转发已启动')
    await refreshStatus()
  } catch (e) {
    const msg = e?.response?.data?.error || e?.response?.data?.msg || e?.message || '启动失败'
    lastError.value = msg
    ElMessage.error(msg)
  } finally {
    starting.value = false
  }
}

async function stopRdp() {
  stopping.value = true
  lastError.value = ''
  try {
    await api.post(`/api/desktop/${props.clientId}/stop`)
    ElMessage.success('已停止')
    form.value.listenPort = 0
    await refreshStatus()
  } catch (e) {
    const msg = e?.response?.data?.error || e?.message || '停止失败'
    lastError.value = msg
    ElMessage.error(msg)
  } finally {
    stopping.value = false
  }
}

async function copyText(text) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    ElMessage.success('已复制')
  } catch (_) {
    ElMessage.warning('复制失败，请手动选择')
  }
}

let pollTimer = null
onMounted(() => {
  refreshStatus()
  pollTimer = setInterval(refreshStatus, 8000)
})
onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer)
})
watch(() => props.clientId, refreshStatus)
</script>

<style scoped>
.remote-desktop {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
  padding: 8px;
}
.rdp-card {
  max-width: 920px;
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.header-tags {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.controls {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-top: 4px;
}
.connect-box h4 {
  margin: 0 0 8px;
}
.hint {
  color: #606266;
  font-size: 13px;
  margin: 0 0 12px;
  line-height: 1.5;
}
.cmd-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
code {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  background: #f5f7fa;
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 13px;
}
</style>
