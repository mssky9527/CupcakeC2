<template>
  <div class="view-shell payload-shell">
    <section class="payload-toolbar">
      <div class="payload-toolbar__metrics">
        <div class="chip">活跃监听器 {{ activeListeners.length }}</div>
        <div class="chip">模式 {{ form.mode === 'build' ? '源码构建' : '模板补丁' }}</div>
      </div>
    </section>

    <section class="payload-grid">
      <article class="surface-card builder-card">
        <div class="panel-head">
          <div>
            <span class="panel-kicker">Build Config</span>
            <h3>载荷参数</h3>
          </div>
          <div class="chip">Core v3</div>
        </div>

        <el-form :model="form" label-position="top" class="payload-form">
          <div class="section-block">
            <div class="section-title">
              <span class="section-index">01</span>
              <div>
                <strong>目标平台</strong>
                <p>统一选择系统与架构，后续会自动同步下载文件名和 stager。</p>
              </div>
            </div>

            <div class="platform-grid">
              <button
                v-for="platform in platformGroups"
                :key="platform.key"
                type="button"
                class="platform-card"
                :class="{ 'platform-card--active': platform.active }"
                @click="form.combinedType = platform.defaultValue"
              >
                <div class="platform-card__head">
                  <div class="platform-card__icon">
                    <el-icon><component :is="platform.icon" /></el-icon>
                  </div>
                  <div>
                    <strong>{{ platform.label }}</strong>
                    <span>{{ platform.caption }}</span>
                  </div>
                </div>

                <el-radio-group v-model="form.combinedType" class="platform-options">
                  <el-radio-button
                    v-for="option in platform.options"
                    :key="option.value"
                    :label="option.value"
                  >
                    {{ option.label }}
                  </el-radio-button>
                </el-radio-group>
              </button>
            </div>
          </div>

          <div class="section-block form-grid">
            <el-form-item label="监听器" required>
              <el-select
                v-model="form.listenerId"
                placeholder="选择一个运行中的监听器"
                @change="onListenerChange"
              >
                <el-option
                  v-for="listener in activeListeners"
                  :key="listener.id"
                  :label="`${listener.protocol} | 端口 ${listener.port}`"
                  :value="listener.id"
                />
              </el-select>
            </el-form-item>

            <el-form-item
              v-if="!isBindTcpListener(selectedListener?.protocol)"
              label="回连地址"
            >
              <el-input
                v-model="form.lhost"
                placeholder="填写公网 IP 或域名"
                :prefix-icon="MapLocation"
              />
            </el-form-item>
          </div>

          <div class="section-block mode-panel">
            <div class="section-title">
              <span class="section-index">02</span>
              <div>
                <strong>生成方式</strong>
                <p>按模板页风格拆分为两种模式，兼顾速度和对抗性。</p>
              </div>
            </div>

            <div class="mode-switch">
              <button
                type="button"
                class="mode-switch__item"
                :class="{ 'mode-switch__item--active': form.mode === 'build' }"
                @click="form.mode = 'build'"
              >
                <span>源码构建</span>
                <small>完整编译，静态链接</small>
              </button>
              <button
                type="button"
                class="mode-switch__item"
                :class="{ 'mode-switch__item--active': form.mode === 'patch' }"
                @click="form.mode = 'patch'"
              >
                <span>模板补丁</span>
                <small>秒级生成，适合快速投递</small>
              </button>
            </div>

            <div class="mode-note">
              <el-icon><Cpu /></el-icon>
              <span>{{ modeDescription }}</span>
            </div>

            <div class="option-grid">
              <div class="option-card">
                <span class="option-card__label">休眠时间</span>
                <strong>{{ form.sleepTime }} 秒</strong>
                <el-input-number v-model="form.sleepTime" :min="0" controls-position="right" />
              </div>

              <div class="option-card">
                <span class="option-card__label">自动销毁</span>
                <strong>{{ form.autoDestruct ? '已启用' : '未启用' }}</strong>
                <el-switch v-model="form.autoDestruct" />
              </div>

              <div class="option-card">
                <span class="option-card__label">UPX 压缩</span>
                <strong>{{ form.useUPX ? '已启用' : '未启用' }}</strong>
                <el-switch v-model="form.useUPX" />
              </div>
            </div>
          </div>

          <div class="build-preview">
            <div class="build-preview__copy">
              <span class="build-preview__label">回连预览</span>
              <code class="build-preview__value">{{ previewUrl }}</code>
            </div>

            <el-button
              type="primary"
              class="generate-btn"
              :loading="loading"
              @click="doGenerate"
            >
              <el-icon v-if="!loading"><Download /></el-icon>
              生成载荷
            </el-button>
          </div>
        </el-form>
      </article>

      <aside class="section-stack payload-sidebar">
        <article class="surface-card sidebar-card">
          <div class="panel-head panel-head--tight">
            <div>
              <span class="panel-kicker">Quick Stager</span>
              <h3>一键上线命令</h3>
            </div>
            <el-button link @click="fetchStagerCommand">
              <el-icon><Refresh /></el-icon>
            </el-button>
          </div>

          <div class="stager-state" v-loading="stagerLoading">
            <template v-if="stagerCommand">
              <div class="terminal-card">
                <div class="terminal-card__dots">
                  <span></span>
                  <span></span>
                  <span></span>
                </div>
                <code>{{ stagerCommand }}</code>
              </div>
              <div class="sidebar-actions">
                <el-button class="sidebar-button" @click="copyStagerCommand">
                  <el-icon><CopyDocument /></el-icon>
                  复制命令
                </el-button>
              </div>
            </template>

            <div v-else class="empty-copy">
              选择监听器和平台后，这里会自动生成对应的快速投递命令。
            </div>
          </div>
        </article>

        <article class="surface-card sidebar-card">
          <div class="panel-head panel-head--tight">
            <div>
              <span class="panel-kicker">Operational Notes</span>
              <h3>投递建议</h3>
            </div>
          </div>

          <div class="tips-stack">
            <div class="tip-row">
              <div class="tip-row__icon">
                <el-icon><Lock /></el-icon>
              </div>
              <p>建议优先选择 WebSocket 监听器，并通过域名或 CDN 出口伪装常规业务流量。</p>
            </div>

            <div class="tip-row">
              <div class="tip-row__icon">
                <el-icon><Share /></el-icon>
              </div>
              <p>如果需要快速大规模投递，优先使用模板补丁模式；需要更强对抗时切回源码构建。</p>
            </div>

            <div class="tip-row">
              <div class="tip-row__icon">
                <el-icon><Monitor /></el-icon>
              </div>
              <p>休眠时间建议保留在 10 到 30 秒区间，兼顾联机体验和自动化沙箱规避。</p>
            </div>
          </div>
        </article>

        <article class="surface-card sidebar-card">
          <div class="panel-head panel-head--tight">
            <div>
              <span class="panel-kicker">Build Status</span>
              <h3>构建状态</h3>
            </div>
            <div class="status-pill" v-if="currentTaskId">
              {{ buildStatusText }}
            </div>
          </div>

          <div class="status-grid">
            <div class="status-cell">
              <span>当前任务</span>
              <strong class="mono">{{ currentTaskId ? currentTaskId.slice(0, 8) : '--------' }}</strong>
            </div>
            <div class="status-cell">
              <span>阶段</span>
              <strong>{{ stageLabel }}</strong>
            </div>
            <div class="status-cell">
              <span>耗时</span>
              <strong class="mono">{{ elapsedTime }}s</strong>
            </div>
          </div>

          <div class="sidebar-actions" v-if="currentTaskId">
            <el-button class="sidebar-button" @click="openBuildConsole">查看控制台</el-button>
            <el-button class="sidebar-button" @click="exportLogs" :disabled="!logBuffer.length">导出日志</el-button>
          </div>
        </article>
      </aside>
    </section>

    <el-dialog
      v-model="terminalDialogVisible"
      width="1040px"
      class="build-dialog premium-dialog"
      destroy-on-close
      @opened="onTerminalOpened"
      @closed="onTerminalClosed"
    >
      <template #header>
        <div class="dialog-header">
          <div>
            <span class="panel-kicker">Build Console</span>
            <h3>任务 {{ currentTaskId ? currentTaskId.slice(0, 8) : '--------' }}</h3>
          </div>

          <div class="dialog-actions">
            <el-button circle plain @click="minimizeTerminal">
              <el-icon><Minus /></el-icon>
            </el-button>
            <el-button circle plain @click="closeBuildSession">
              <el-icon><Close /></el-icon>
            </el-button>
          </div>
        </div>
      </template>

      <div class="dialog-content">
        <div class="status-grid status-grid--dialog">
          <div class="status-cell">
            <span>状态</span>
            <strong>{{ buildStatusText }}</strong>
          </div>
          <div class="status-cell">
            <span>阶段</span>
            <strong>{{ stageLabel }}</strong>
          </div>
          <div class="status-cell">
            <span>目标架构</span>
            <strong class="mono">{{ form.combinedType }}</strong>
          </div>
          <div class="status-cell">
            <span>耗时</span>
            <strong class="mono">{{ elapsedTime }}s</strong>
          </div>
        </div>

        <div class="pipeline">
          <div
            v-for="step in buildSteps"
            :key="step.id"
            class="pipeline-step"
            :class="{
              'pipeline-step--active': buildStage >= step.id,
              'pipeline-step--done': buildStage > step.id
            }"
          >
            <div class="pipeline-step__dot">{{ step.id }}</div>
            <span>{{ step.label }}</span>
          </div>
        </div>

        <div class="terminal-toolbar">
          <span>实时构建输出</span>
          <div class="terminal-toolbar__actions">
            <el-button link @click="exportLogs">导出日志</el-button>
            <el-button link @click="clearTerminal">清空缓冲</el-button>
          </div>
        </div>

        <div class="terminal-wrap">
          <div ref="terminalContainer" class="xterm-view"></div>
        </div>
      </div>
    </el-dialog>

    <transition name="pop">
      <button
        v-if="isMinimized && currentTaskId"
        type="button"
        class="build-bubble"
        @click="restoreTerminal"
      >
        <el-icon><Cpu /></el-icon>
        <div>
          <strong>{{ buildFinished ? '构建结果已返回' : '构建任务进行中' }}</strong>
          <span>ID {{ currentTaskId.slice(0, 8) }} · {{ buildStatusText }}</span>
        </div>
      </button>
    </transition>
  </div>
</template>

<script setup>
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  ChromeFilled,
  Close,
  CopyDocument,
  Cpu,
  Download,
  Lock,
  MapLocation,
  Minus,
  Monitor,
  Platform,
  Refresh,
  Share
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getListeners, generateClient, request } from '@/api'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'

const loading = ref(false)
const activeListeners = ref([])
const stagerLoading = ref(false)
const stagerCommand = ref('')

const currentTaskId = ref('')
const terminalDialogVisible = ref(false)
const isMinimized = ref(false)
const buildStage = ref(1)
const buildStatusText = ref('等待任务')
const elapsedTime = ref(0)
const logBuffer = ref([])
const buildFinished = ref(false)

const terminalContainer = ref(null)

let xterm = null
let fitAddon = null
let ws = null
let buildTimer = null
let resizeHandler = null

const form = ref({
  combinedType: 'windows_amd64',
  listenerId: '',
  lhost: window.location.hostname || '127.0.0.1',
  mode: 'build',
  autoDestruct: false,
  sleepTime: 0,
  aesKey: '',
  useUPX: false,
  encryption_salt: '',
  obfuscation_mode: 'none'
})

const platformGroups = computed(() => [
  {
    key: 'windows',
    label: 'Windows',
    caption: '桌面与服务器环境',
    icon: Platform,
    defaultValue: 'windows_amd64',
    active: form.value.combinedType.startsWith('windows'),
    options: [
      { label: 'X64 标准版', value: 'windows_amd64' },
      { label: 'X86 兼容版', value: 'windows_i386' }
    ]
  },
  {
    key: 'linux',
    label: 'Linux',
    caption: '常规服务器与 ARM',
    icon: ChromeFilled,
    defaultValue: 'linux_amd64',
    active: form.value.combinedType.startsWith('linux'),
    options: [
      { label: 'AMD64', value: 'linux_amd64' },
      { label: 'ARM64 / M1', value: 'linux_arm64' }
    ]
  }
])

const buildSteps = [
  { id: 1, label: '环境检查' },
  { id: 2, label: '核心编译' },
  { id: 3, label: '压缩封装' }
]

const selectedListener = computed(() =>
  activeListeners.value.find((listener) => listener.id === form.value.listenerId)
)

const isBindTcpListener = (protocol) => {
  const value = String(protocol || '').toLowerCase()
  return value === '正向tcp' || value === 'bind-tcp' || value === 'bind_tcp' || value.includes('bind')
}

const previewUrl = computed(() => {
  if (!selectedListener.value) return '---'

  const protocol = (selectedListener.value.protocol || '').toLowerCase()
  if (protocol === 'websocket') return `ws://${form.value.lhost}:${selectedListener.value.port}/ws`
  if (isBindTcpListener(protocol)) return `LOCAL_BIND:${selectedListener.value.port}`
  if (protocol === 'dns') return `NS:${selectedListener.value.ns_domain}`
  return `${selectedListener.value.protocol}://${form.value.lhost}:${selectedListener.value.port}`
})

const modeDescription = computed(() => (
  form.value.mode === 'build'
    ? '调用远程 Rust 构建链路，适合需要完整静态编译与更高对抗性的交付场景。'
    : '基于预编译模板快速打补丁，适合需要秒级生成和批量分发的场景。'
))

const stageLabel = computed(() => {
  if (buildStage.value <= 1) return '环境检查'
  if (buildStage.value === 2) return '核心编译'
  if (buildStage.value === 3) return '压缩封装'
  return buildFinished.value ? '已完成' : '处理中'
})

watch(
  [() => form.value.combinedType, () => form.value.listenerId, () => form.value.lhost],
  () => {
    fetchStagerCommand()
  }
)

const syncBuildTimer = (running) => {
  window.clearInterval(buildTimer)
  if (running) {
    buildTimer = window.setInterval(() => {
      elapsedTime.value += 1
    }, 1000)
  }
}

const hydrateTerminal = () => {
  if (!terminalContainer.value) return

  xterm = new XTerm({
    theme: {
      background: '#0f0f10',
      foreground: '#f2f2f2',
      cursor: '#ffffff'
    },
    fontSize: 13,
    fontFamily: 'Consolas, SFMono-Regular, monospace',
    convertEol: true
  })

  fitAddon = new FitAddon()
  xterm.loadAddon(fitAddon)
  xterm.open(terminalContainer.value)
  fitAddon.fit()

  if (logBuffer.value.length) {
    xterm.write(logBuffer.value.join('\r\n'))
    xterm.write('\r\n')
  }
}

const disposeTerminal = () => {
  if (xterm) {
    xterm.dispose()
    xterm = null
  }
  fitAddon = null
}

const pushTerminalLog = (line) => {
  logBuffer.value.push(line)
  if (xterm) {
    xterm.writeln(line)
  }
}

const openBuildConsole = async () => {
  if (!currentTaskId.value) return
  isMinimized.value = false
  terminalDialogVisible.value = true
  await nextTick()
}

const restoreTerminal = () => {
  openBuildConsole()
}

const minimizeTerminal = () => {
  isMinimized.value = true
  terminalDialogVisible.value = false
}

const closeBuildSocket = () => {
  if (ws) {
    ws.close()
    ws = null
  }
}

const closeBuildSession = () => {
  terminalDialogVisible.value = false
  isMinimized.value = false
  closeBuildSocket()
  syncBuildTimer(false)
  disposeTerminal()
  currentTaskId.value = ''
  buildFinished.value = false
  buildStatusText.value = '等待任务'
  buildStage.value = 1
  elapsedTime.value = 0
}

const onTerminalOpened = async () => {
  await nextTick()
  disposeTerminal()
  hydrateTerminal()
}

const onTerminalClosed = () => {
  disposeTerminal()
  if (!isMinimized.value && buildFinished.value) {
    closeBuildSocket()
  }
}

const clearTerminal = () => {
  logBuffer.value = []
  xterm?.clear()
}

const exportLogs = () => {
  if (!logBuffer.value.length) return
  const blob = new Blob([logBuffer.value.join('\n')], { type: 'text/plain' })
  const link = document.createElement('a')
  link.href = URL.createObjectURL(blob)
  link.download = `build_${currentTaskId.value ? currentTaskId.value.slice(0, 8) : 'logs'}.txt`
  link.click()
}

const downloadArtifact = async (url) => {
  const response = await request.get(url, { responseType: 'blob' })
  const disposition = response.headers['content-disposition'] || ''
  let filename = disposition.match(/filename\*?=['"]?([^;\n"']+)/i)?.[1] || url.split('/').pop()
  if (filename.includes("''")) {
    filename = decodeURIComponent(filename.replace(/.*''/, ''))
  }
  const link = document.createElement('a')
  link.href = URL.createObjectURL(response.data)
  link.download = filename
  link.click()
}

const attachBuildSocket = () => {
  const configuredBase = import.meta.env.VITE_API_BASE_URL || ''
  let socketBase = `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}`

  if (configuredBase && configuredBase !== '/') {
    if (configuredBase.startsWith('http://') || configuredBase.startsWith('https://')) {
      socketBase = configuredBase.replace(/^http/, 'ws')
    } else {
      const normalizedBase = configuredBase.startsWith('/') ? configuredBase : `/${configuredBase}`
      socketBase = `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}${normalizedBase}`
    }
  }

  const token = localStorage.getItem('cupcake_token')

  closeBuildSocket()
  ws = new WebSocket(`${socketBase}/api/build/logs/${currentTaskId.value}?token=${token}`)

  ws.onmessage = async (event) => {
    const payload = JSON.parse(event.data)

    if (payload.type === 'log') {
      pushTerminalLog(payload.content)
      const text = String(payload.content).toLowerCase()
      if (text.includes('cargo') || text.includes('compiling')) {
        buildStage.value = 2
        buildStatusText.value = '正在编译核心'
      } else if (text.includes('upx')) {
        buildStage.value = 3
        buildStatusText.value = '正在压缩封装'
      }
      return
    }

    if (payload.type === 'success') {
      pushTerminalLog(`[OK] ${payload.content}`)
      buildStage.value = 4
      buildStatusText.value = '构建成功'
      buildFinished.value = true
      syncBuildTimer(false)
      await downloadArtifact(payload.content)
      return
    }

    if (payload.type === 'error') {
      pushTerminalLog(`[FAIL] ${payload.content}`)
      buildStatusText.value = '构建失败'
      buildFinished.value = true
      syncBuildTimer(false)
    }
  }
}

const fetchListenersData = async () => {
  try {
    const response = await getListeners()
    activeListeners.value = (response.data || []).filter((listener) => listener.status === 'Running')
    if (!form.value.listenerId && activeListeners.value.length > 0) {
      form.value.listenerId = activeListeners.value[0].id
      onListenerChange(form.value.listenerId)
    }
  } catch {
    ElMessage.error('无法加载监听器列表')
  }
}

const onListenerChange = (id) => {
  const listener = activeListeners.value.find((item) => item.id === id)
  if (!listener) return
  form.value.aesKey = listener.encrypt_key || ''
  form.value.encryption_salt = listener.encryption_salt || ''
  form.value.obfuscation_mode = listener.obfuscate_mode || 'none'
}

const handleDirectDownload = (response) => {
  const blob = response.data
  const disposition = response.headers['content-disposition'] || ''
  let filename = disposition.match(/filename\*?=['"]?([^;\n"']+)/i)?.[1] || ''
  if (filename.includes("''")) {
    filename = decodeURIComponent(filename.replace(/.*''/, ''))
  }
  if (!filename) {
    const os = form.value.combinedType.split('_')[0]
    const ext = os === 'windows' ? '.exe' : ''
    filename = `agent_${form.value.combinedType}${ext}`
  }
  const link = document.createElement('a')
  link.href = URL.createObjectURL(blob)
  link.download = filename
  link.click()
}

const doGenerate = async () => {
  if (!form.value.listenerId) {
    ElMessage.warning('请先选择监听器')
    return
  }

  loading.value = true
  try {
    const payload = {
      os: form.value.combinedType.split('_')[0],
      arch: form.value.combinedType,
      listener_id: form.value.listenerId,
      host: form.value.lhost,
      method: form.value.mode,
      auto_destruct: form.value.autoDestruct,
      sleep_time: form.value.sleepTime,
      aes_key: form.value.aesKey,
      use_upx: form.value.useUPX,
      encryption_salt: form.value.encryption_salt,
      obfuscation_mode: form.value.obfuscation_mode
    }

    const response = await generateClient(payload)
    const blobData = response.data

    if (blobData.type === 'application/json' || blobData.size < 2048) {
      const text = await blobData.text()
      const json = JSON.parse(text)
      if (json.task_id) {
        currentTaskId.value = json.task_id
        logBuffer.value = []
        elapsedTime.value = 0
        buildStage.value = 1
        buildStatusText.value = '正在准备构建环境'
        buildFinished.value = false
        terminalDialogVisible.value = true
        isMinimized.value = false
        syncBuildTimer(true)
        attachBuildSocket()
        return
      }
    }

    handleDirectDownload(response)
    ElMessage.success('载荷已生成并开始下载')
  } catch {
    ElMessage.error('生成失败，请检查监听器与构建配置')
  } finally {
    loading.value = false
  }
}

const fetchStagerCommand = async () => {
  if (!form.value.listenerId) {
    stagerCommand.value = ''
    return
  }

  stagerLoading.value = true
  try {
    const os = form.value.combinedType.split('_')[0]
    const response = await request.get('/api/stager', {
      params: {
        listener_id: form.value.listenerId,
        os,
        host: form.value.lhost
      }
    })
    stagerCommand.value = response.data.command
  } catch {
    stagerCommand.value = ''
  } finally {
    stagerLoading.value = false
  }
}

const copyStagerCommand = async () => {
  if (!stagerCommand.value) return
  await navigator.clipboard.writeText(stagerCommand.value)
  ElMessage.success('命令已复制到剪贴板')
}

onMounted(async () => {
  await fetchListenersData()

  resizeHandler = () => {
    fitAddon?.fit()
  }
  window.addEventListener('resize', resizeHandler)
})

onUnmounted(() => {
  window.removeEventListener('resize', resizeHandler)
  syncBuildTimer(false)
  closeBuildSocket()
  disposeTerminal()
})
</script>

<style scoped>
.payload-shell {
  flex: 1;
  min-height: 0;
  gap: 20px;
}

.payload-toolbar {
  display: flex;
  justify-content: flex-end;
}

.payload-toolbar__metrics {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.payload-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.45fr) minmax(320px, 0.8fr);
  gap: 20px;
  min-height: 0;
}

.builder-card,
.sidebar-card {
  padding: 24px;
}

.panel-head--tight {
  margin-bottom: 16px;
}
.panel-head h3,
.dialog-header h3 {
  margin: 0;
  font-size: 24px;
  letter-spacing: -0.04em;
}

.payload-form {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.section-block {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.section-title {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}

.section-index {
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
  border-radius: 12px;
  background: var(--surface-muted);
  color: var(--text-strong);
  font-size: 12px;
  font-weight: 800;
}

.section-title strong {
  display: block;
  margin-bottom: 4px;
  font-size: 15px;
}

.section-title p {
  margin: 0;
  color: var(--text-body);
  line-height: 1.6;
  font-size: 13px;
}

.platform-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}

.platform-card {
  padding: 20px;
  border: 1px solid var(--line-soft);
  border-radius: 22px;
  background: var(--surface-soft);
  text-align: left;
  cursor: pointer;
  transition: transform 0.16s ease, border-color 0.16s ease, background 0.16s ease;
}

.platform-card:hover {
  transform: translateY(-1px);
  border-color: var(--line-strong);
}

.platform-card--active {
  background: #ffffff;
  border-color: var(--text-strong);
  box-shadow: var(--shadow-soft);
}

.platform-card__head {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 14px;
}

.platform-card__icon {
  width: 42px;
  height: 42px;
  display: grid;
  place-items: center;
  border-radius: 14px;
  background: var(--surface-muted);
  font-size: 18px;
}

.platform-card__head strong,
.platform-card__head span {
  display: block;
}

.platform-card__head span {
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-muted);
}

.platform-options {
  width: 100%;
}

.platform-options :deep(.el-radio-button) {
  flex: 1;
}

.platform-options :deep(.el-radio-button__inner) {
  width: 100%;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px;
}

.mode-panel {
  padding: 22px;
  border-radius: 24px;
  background: var(--surface-soft);
  border: 1px solid var(--line-soft);
}

.mode-switch {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.mode-switch__item {
  padding: 16px 18px;
  border: 1px solid var(--line-soft);
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.82);
  text-align: left;
  cursor: pointer;
}

.mode-switch__item span,
.mode-switch__item small {
  display: block;
}

.mode-switch__item span {
  font-weight: 800;
  color: var(--text-strong);
}

.mode-switch__item small {
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-muted);
}

.mode-switch__item--active {
  border-color: var(--text-strong);
  background: #ffffff;
}

.mode-note {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 16px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.8);
  color: var(--text-body);
  line-height: 1.6;
  font-size: 13px;
}

.option-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
}

.option-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  border-radius: 18px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.9);
}

.option-card__label {
  font-size: 11px;
  font-weight: 800;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: var(--text-muted);
}

.option-card strong {
  font-size: 16px;
}

.build-preview {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding-top: 6px;
  border-top: 1px solid var(--line-soft);
}

.build-preview__copy {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.build-preview__label {
  font-size: 11px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.12em;
  font-weight: 700;
}

.build-preview__value {
  color: var(--text-strong);
  font-size: 13px;
  word-break: break-all;
}

.generate-btn {
  min-width: 150px;
}

.stager-state,
.tips-stack,
.status-grid {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.terminal-card {
  padding: 16px;
  border-radius: 18px;
  background: #0f0f10;
  color: #f2f2f2;
}

.terminal-card__dots {
  display: flex;
  gap: 6px;
  margin-bottom: 12px;
}

.terminal-card__dots span {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.26);
}

.terminal-card code {
  display: block;
  line-height: 1.7;
  word-break: break-all;
  font-size: 12px;
  font-family: Consolas, SFMono-Regular, monospace;
}

.empty-copy {
  padding: 22px 0 4px;
  font-size: 13px;
  color: var(--text-muted);
  line-height: 1.7;
}

.sidebar-actions {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.sidebar-button {
  flex: 1;
  min-width: 120px;
}

.tip-row {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}

.tip-row__icon {
  width: 38px;
  height: 38px;
  display: grid;
  place-items: center;
  border-radius: 14px;
  background: var(--surface-muted);
  color: var(--text-strong);
  flex-shrink: 0;
}

.tip-row p {
  margin: 0;
  font-size: 13px;
  color: var(--text-body);
  line-height: 1.7;
}

.status-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.status-grid--dialog {
  grid-template-columns: repeat(4, minmax(0, 1fr));
}

.status-cell {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 14px 16px;
  border-radius: 18px;
  background: var(--surface-soft);
  border: 1px solid var(--line-soft);
}

.status-cell span {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: var(--text-muted);
}

.status-cell strong {
  font-size: 14px;
}

.dialog-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 18px;
}

.dialog-actions {
  display: flex;
  gap: 10px;
}

.dialog-content {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.pipeline {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.pipeline-step {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 16px;
  border-radius: 18px;
  background: var(--surface-soft);
  border: 1px solid var(--line-soft);
  color: var(--text-muted);
  font-size: 13px;
  font-weight: 700;
}

.pipeline-step__dot {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  border-radius: 999px;
  background: #ffffff;
  border: 1px solid var(--line-soft);
  font-size: 11px;
}

.pipeline-step--active,
.pipeline-step--done {
  color: var(--text-strong);
}

.pipeline-step--active {
  border-color: var(--text-strong);
}

.pipeline-step--active .pipeline-step__dot {
  background: var(--text-strong);
  color: #ffffff;
  border-color: var(--text-strong);
}

.pipeline-step--done .pipeline-step__dot {
  border-color: var(--text-strong);
}

.terminal-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 0 4px;
}

.terminal-toolbar span {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.12em;
}

.terminal-toolbar__actions {
  display: flex;
  gap: 10px;
}

.terminal-wrap {
  height: 420px;
  padding: 16px;
  border-radius: 22px;
  background: #0f0f10;
}

.xterm-view {
  width: 100%;
  height: 100%;
}

.build-bubble {
  position: fixed;
  right: 28px;
  bottom: 28px;
  display: inline-flex;
  align-items: center;
  gap: 14px;
  padding: 14px 18px;
  border: 0;
  border-radius: 20px;
  background: #111111;
  color: #ffffff;
  box-shadow: 0 16px 40px rgba(17, 17, 17, 0.18);
  cursor: pointer;
  z-index: 2100;
}

.build-bubble strong,
.build-bubble span {
  display: block;
  text-align: left;
}

.build-bubble strong {
  font-size: 13px;
}

.build-bubble span {
  margin-top: 4px;
  font-size: 11px;
  opacity: 0.72;
}

.pop-enter-active,
.pop-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translateY(8px);
}

@media (max-width: 1240px) {
  .payload-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 900px) {
  .payload-toolbar__metrics {
    width: 100%;
  }

  .platform-grid,
  .form-grid,
  .option-grid,
  .status-grid,
  .status-grid--dialog,
  .pipeline {
    grid-template-columns: 1fr;
  }

  .build-preview {
    flex-direction: column;
    align-items: stretch;
  }

  .generate-btn,
  .sidebar-button {
    width: 100%;
  }
}

@media (max-width: 720px) {
  .payload-toolbar {
    flex-direction: column;
    align-items: stretch;
  }

  .builder-card,
  .sidebar-card,
  .terminal-wrap {
    padding: 18px;
  }

  .panel-head h3,
  .dialog-header h3 {
    font-size: 20px;
  }

  .mode-switch {
    grid-template-columns: 1fr;
  }

  .dialog-header,
  .terminal-toolbar {
    flex-direction: column;
    align-items: flex-start;
  }

  .build-bubble {
    left: 14px;
    right: 14px;
    bottom: 14px;
  }
}
</style>
