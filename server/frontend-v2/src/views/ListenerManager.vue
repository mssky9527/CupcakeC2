<template>
  <div class="listener-manager-container">
    <!-- Page Header (Bento Box style) -->
    <div class="page-header glass-panel mb-24">
      <div class="header-content">
        <div class="title-section">
          <h2 class="main-title">通信链路 <span class="purple-text">监听管理</span></h2>
          <p class="sub-title">多协议指挥通道</p>
        </div>
        <div class="action-section">
          <el-button class="premium-btn create-btn" type="primary" :icon="Plus" @click="openCreateDialog">
            启动新监听链路
          </el-button>
          <el-button class="premium-btn refresh-btn" :loading="loading" plain @click="fetchListeners">
            <el-icon><Refresh /></el-icon>
          </el-button>
        </div>
      </div>
    </div>

    <!-- Stats Matrix (Quick Info) -->
    <div class="stats-row mb-24">
      <div class="stat-module glass-panel">
        <div class="stat-icon-box purple">
          <el-icon><Headset /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">活跃链路</div>
          <div class="stat-value">{{ listeners.filter(l => l.status === 'Running').length }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box green">
          <el-icon><Connection /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">通信承载</div>
          <div class="stat-value">{{ listeners.length }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box blue">
          <el-icon><Monitor /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">总吞吐量</div>
          <div class="stat-value">--</div>
        </div>
      </div>
    </div>

    <!-- Table Section -->
    <div class="table-module glass-panel">
      <el-table :data="listeners" class="premium-table" v-loading="loading">
        <el-table-column width="60" align="center">
           <template #default="scope">
             <div class="protocol-icon" :class="scope.row.protocol.toLowerCase()">
                <el-icon v-if="scope.row.protocol === 'TCP'"><Share /></el-icon>
                <el-icon v-else-if="scope.row.protocol === 'WebSocket'"><Connection /></el-icon>
                <el-icon v-else><Monitor /></el-icon>
             </div>
           </template>
        </el-table-column>

        <el-table-column prop="protocol" label="协议模板" width="130">
          <template #default="scope">
            <el-tag :type="getProtocolType(scope.row.protocol)" class="premium-tag" effect="plain" round>
              {{ scope.row.protocol }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column label="侦听地址" min-width="180">
          <template #default="scope">
            <code class="addr-code">{{ scope.row.bind_ip || '0.0.0.0' }}:{{ scope.row.port }}</code>
          </template>
        </el-table-column>

        <el-table-column prop="note" label="备注说明" min-width="150" show-overflow-tooltip>
          <template #default="scope">
            <span class="note-text">{{ scope.row.note || '---' }}</span>
          </template>
        </el-table-column>

        <el-table-column label="链路状态" width="120" align="center">
          <template #default="scope">
            <div class="status-indicator" :class="scope.row.status.toLowerCase()">
              <span class="dot"></span>
              {{ scope.row.status === 'Running' ? '工作中' : '已停止' }}
            </div>
          </template>
        </el-table-column>

        <el-table-column label="管理维护" width="300" align="center" fixed="right">
          <template #default="scope">
            <el-button 
              type="primary" 
              link 
              class="action-btn purple"
              @click="openStagerDialog(scope.row)"
            >
              一键上线
            </el-button>
            <el-divider direction="vertical" />
            <el-button 
              v-if="scope.row.status === 'Stopped' || scope.row.status === 'Failed'"
              link
              class="action-btn green"
              @click="handleStart(scope.row.id)"
            >
              激活
            </el-button>
            <el-button 
              v-else
              link
              class="action-btn orange"
              @click="handleStop(scope.row.id)"
            >
              熔断
            </el-button>
            <el-button 
              link
              class="action-btn red"
              @click="handleDelete(scope.row.id)"
            >
              销毁
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- One-click On-boarding Dialog -->
    <el-dialog v-model="stagerVisible" title="指令注入 - 一键上线" width="620px" class="premium-dialog" center>
      <div class="dialog-inner">
        <div class="platform-tabs mb-20">
          <el-radio-group v-model="stagerPlatform" size="large" @change="fetchStager">
            <el-radio-button label="windows">WINDOWS (PS)</el-radio-button>
            <el-radio-button label="linux">LINUX (BASH)</el-radio-button>
          </el-radio-group>
        </div>
        <div v-loading="stagerLoading">
          <p class="dialog-hint">在目标机器执行以下指令完成资产同步：</p>
          <div class="terminal-box">
            <div class="term-header">
               <span class="dot red"></span>
               <span class="dot yellow"></span>
               <span class="dot green"></span>
            </div>
            <pre><code>{{ stagerCommand }}</code></pre>
            <el-button class="copy-btn-mini" @click="copyCommand">
              <el-icon><CopyDocument /></el-icon> 复制
            </el-button>
          </div>
        </div>
      </div>
      <template #footer>
        <el-button @click="stagerVisible = false" class="plain-btn">关闭</el-button>
      </template>
    </el-dialog>

    <!-- Professional configuration -->
    <el-dialog 
      v-model="dialogVisible" 
      title="配置核心通信模板" 
      width="700px" 
      class="premium-dialog"
      destroy-on-close
    >
      <el-form :model="form" label-position="top">
        <div class="form-grid">
           <div class="form-aside">
              <div class="protocol-selector-v2">
                 <div 
                   v-for="p in ['TCP', 'WebSocket', '正向TCP', 'DNS']" 
                   :key="p"
                   class="p-item"
                   :class="{ active: form.protocol === p }"
                   @click="form.protocol = p; handleProtocolChange(p)"
                 >
                   {{ p }}
                 </div>
              </div>
           </div>
           <div class="form-main">
              <el-row :gutter="20">
                <el-col :span="14">
                   <el-form-item :label="form.protocol === '正向TCP' ? '监听端口' : '侦听地址 (IP:Port)'">
                      <el-input v-model="listenAddr" :placeholder="form.protocol === '正向TCP' ? '4444' : '0.0.0.0:8081'" />
                   </el-form-item>
                </el-col>
                <el-col :span="10">
                   <el-form-item label="名称/别名">
                      <el-input v-model="form.note" placeholder="链路描述" />
                   </el-form-item>
                </el-col>
              </el-row>

              <el-form-item label="公开投递Host (Public C2 Host)" v-if="form.protocol !== '正向TCP'">
                <el-input v-model="form.public_host" placeholder="c2.example.com" />
              </el-form-item>

              <el-row :gutter="20" v-if="form.protocol === 'DNS'">
                <el-col :span="24">
                  <el-form-item label="NS Domain delegation" required>
                    <el-input v-model="form.ns_domain" placeholder="ns1.corp.com" />
                  </el-form-item>
                </el-col>
              </el-row>

              <div class="advanced-section glass-panel">
                 <label class="section-label">安全与混淆安全 (Security Settings)</label>
                 <el-row :gutter="20">
                    <el-col :span="24">
                       <el-form-item label="Vkey (通讯秘钥)">
                          <el-input v-model="form.encrypt_key" :type="showKey ? 'text' : 'password'" class="vkey-input">
                             <template #append>
                               <el-button @click="generateKey" class="random-btn">随机生成</el-button>
                             </template>
                          </el-input>
                       </el-form-item>
                    </el-col>
                    <el-col :span="12">
                       <el-form-item label="加密盐值 (Salt)">
                          <el-input v-model="form.encryption_salt" />
                       </el-form-item>
                    </el-col>
                    <el-col :span="12">
                       <el-form-item label="报文编码方式">
                          <el-select v-model="form.obfuscate_mode" style="width: 100%">
                            <el-option label="None (Plain)" value="None" />
                            <el-option label="Base64" value="Base64" />
                            <el-option label="XOR Stream" value="XOR" />
                          </el-select>
                       </el-form-item>
                    </el-col>
                 </el-row>
              </div>
           </div>
        </div>
      </el-form>

      <template #footer>
        <div class="dialog-footer-v2">
          <span class="warning-text">* 防火墙需预先放行相应流量端口</span>
          <div class="btns">
            <el-button @click="dialogVisible = false" class="plain-btn">取消</el-button>
            <el-button type="primary" class="purple-btn" :loading="submitting" @click="createListener">下发部署指令</el-button>
          </div>
        </div>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, reactive, onMounted } from 'vue'
import api from '../api/index'
import { ElMessage, ElMessageBox } from 'element-plus'
import { 
  Plus, Connection, Monitor, Lock, Setting, Promotion, 
  View, Hide, Refresh, CopyDocument, Share, Headset
} from '@element-plus/icons-vue'

const listeners = ref([])
const loading = ref(false)
const dialogVisible = ref(false)
const submitting = ref(false)
const showKey = ref(false)
const listenAddr = ref('0.0.0.0:8081')

// Stager State
const stagerVisible = ref(false)
const stagerLoading = ref(false)
const stagerCommand = ref('')
const stagerPlatform = ref('windows')
const currentListener = ref(null)

const form = reactive({
  bind_ip: '0.0.0.0',
  port: 8081,
  note: '',
  protocol: 'WebSocket',
  public_host: '',
  encrypt_mode: 'AES-256-GCM',
  encrypt_key: '',
  encryption_salt: '',
  obfuscate_mode: 'None',
  ns_domain: '',
  public_dns: '8.8.8.8:53',
  heartbeat_mode: 'auto',
  heartbeat_interval: 10,
  max_retry: 30
})

const fetchListeners = async () => {
  loading.value = true
  try {
    const res = await api.get('/api/listeners')
    listeners.value = res.data || []
  } catch (e) { ElMessage.error('无法同步链路') }
  finally { loading.value = false }
}

const openCreateDialog = () => {
  dialogVisible.value = true
  generateKey()
  generateRandomSalt()
  handleProtocolChange(form.protocol)
}

const handleProtocolChange = (val) => {
  if (val === 'DNS') listenAddr.value = '0.0.0.0:53'
  else if (val === 'WebSocket') listenAddr.value = '0.0.0.0:8081'
  else if (val === 'TCP') listenAddr.value = '0.0.0.0:8888'
  else if (val === '正向TCP') listenAddr.value = '4444'
}

const generateKey = () => {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
  let key = ''
  for (let i = 0; i < 32; i++) key += chars.charAt(Math.floor(Math.random() * chars.length))
  form.encrypt_key = key
  showKey.value = true
}

const generateRandomSalt = () => {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
  let result = ''
  for (let i = 0; i < 6; i++) result += chars.charAt(Math.floor(Math.random() * chars.length))
  form.encryption_salt = result
}

const getProtocolType = (p) => {
  const map = { 'WebSocket': 'success', 'TCP': 'primary', '正向TCP': 'warning', 'DNS': 'warning' }
  return map[p] || 'info'
}

const createListener = async () => {
  const parts = listenAddr.value.split(':')
  if (parts.length === 2) {
    form.bind_ip = parts[0] || '0.0.0.0'
    form.port = parseInt(parts[1])
  } else if (!isNaN(listenAddr.value)) {
    form.bind_ip = '0.0.0.0'
    form.port = parseInt(listenAddr.value)
  }

  submitting.value = true
  try {
    await api.post('/api/listeners', { ...form })
    ElMessage.success('链路部署成功')
    dialogVisible.value = false
    fetchListeners()
  } catch (e) { ElMessage.error('部署失败') }
  finally { submitting.value = false }
}

const handleStop = async (id) => {
  await api.post(`/api/listeners/${id}/stop`)
  ElMessage.warning('链路已熔断')
  fetchListeners()
}

const handleStart = async (id) => {
  await api.post(`/api/listeners/${id}/start`)
  ElMessage.success('链路已重连')
  fetchListeners()
}

const handleDelete = (id) => {
  ElMessageBox.confirm('确定销毁该链路吗？', '销毁确认', { type: 'error' }).then(async () => {
    await api.delete(`/api/listeners/${id}`)
    ElMessage.success('已销毁')
    fetchListeners()
  })
}

const openStagerDialog = (row) => {
  currentListener.value = row
  stagerVisible.value = true
  fetchStager()
}

const fetchStager = async () => {
  stagerLoading.value = true
  try {
    const res = await api.get('/api/stager', {
      params: {
        listener_id: currentListener.value.id,
        os: stagerPlatform.value,
        arch: 'x64',
        host: currentListener.value.public_host || window.location.hostname
      }
    })
    stagerCommand.value = res.data.command
  } catch (e) { ElMessage.error('生成失败') }
  finally { stagerLoading.value = false }
}

const copyCommand = () => {
  navigator.clipboard.writeText(stagerCommand.value)
  ElMessage.success('指令已复制')
}

onMounted(fetchListeners)
</script>

<style scoped>
.listener-manager-container { padding: 0; animation: fadeIn 0.5s ease-out; }
@keyframes fadeIn { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: translateY(0); } }

.mb-24 { margin-bottom: 24px; }
.mb-20 { margin-bottom: 20px; }

/* Bento Panes */
.glass-panel {
  background: rgba(255, 255, 255, 0.75);
  backdrop-filter: blur(20px); -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(124, 58, 237, 0.08); border-radius: 24px;
  box-shadow: 0 10px 30px rgba(124, 58, 237, 0.05);
}

.page-header { padding: 24px 32px; }
.header-content { display: flex; justify-content: space-between; align-items: center; }
.main-title { font-size: 26px; font-weight: 900; color: #1e1b4b; margin: 0; }
.purple-text { color: #7c3aed; }
.sub-title { font-size: 13px; color: #94a3b8; font-weight: 600; margin-top: 4px; }

.premium-btn { border-radius: 12px; font-weight: 700; height: 42px; transition: all 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275); }
.create-btn { background: #7c3aed !important; border: none !important; box-shadow: 0 4px 15px rgba(124, 58, 237, 0.3); }
.create-btn:hover { transform: translateY(-2px); box-shadow: 0 8px 25px rgba(124, 58, 237, 0.4); }

/* Stats Matrix */
.stats-row { display: grid; grid-template-columns: repeat(3, 1fr); gap: 20px; }
.stat-module { padding: 20px; display: flex; align-items: center; gap: 16px; }
.stat-icon-box { width: 48px; height: 48px; border-radius: 14px; display: flex; align-items: center; justify-content: center; font-size: 22px; }
.stat-icon-box.purple { background: rgba(124, 58, 237, 0.1); color: #7c3aed; }
.stat-icon-box.green { background: rgba(16, 185, 129, 0.1); color: #10b981; }
.stat-icon-box.blue { background: rgba(14, 165, 233, 0.1); color: #0ea5e9; }
.stat-label { font-size: 11px; font-weight: 800; color: #94a3b8; text-transform: uppercase; letter-spacing: 0.5px; }
.stat-value { font-family: 'JetBrains Mono'; font-size: 26px; font-weight: 800; color: #1e1b4b; }

/* Table */
.table-module { padding: 12px; }
.premium-table { background: transparent !important; }

.protocol-icon { width: 32px; height: 32px; border-radius: 8px; display: flex; align-items: center; justify-content: center; font-size: 16px; }
.protocol-icon.tcp { background: #eff6ff; color: #3b82f6; }
.protocol-icon.websocket { background: #f0fdf4; color: #22c55e; }
.protocol-icon.dns { background: #fffbeb; color: #d97706; }

.addr-code { font-family: 'JetBrains Mono'; font-weight: 700; color: #7c3aed; background: rgba(124, 58, 237, 0.05); padding: 4px 10px; border-radius: 8px; font-size: 13px; }
.note-text { color: #64748b; font-size: 13px; font-weight: 600; }

.status-indicator { display: inline-flex; align-items: center; gap: 8px; padding: 4px 12px; border-radius: 20px; font-size: 11px; font-weight: 800; }
.status-indicator.working, .status-indicator.running { background: rgba(16, 185, 129, 0.1); color: #059669; }
.status-indicator.stopped { background: #f1f5f9; color: #94a3b8; }
.status-indicator.working .dot, .status-indicator.running .dot { width: 6px; height: 6px; background: #10b981; border-radius: 50%; box-shadow: 0 0 8px #10b981; }

.action-btn { font-size: 13px; font-weight: 800; }
.action-btn.purple { color: #7c3aed !important; }
.action-btn.green { color: #10b981 !important; }
.action-btn.orange { color: #f59e0b !important; }
.action-btn.red { color: #ef4444 !important; }

/* Terminal Stager */
.terminal-box { background: #0f172a; padding: 20px; border-radius: 16px; position: relative; border: 1px solid rgba(124, 58, 237, 0.2); }
.term-header { display: flex; gap: 6px; margin-bottom: 12px; }
.term-header .dot { width: 8px; height: 8px; border-radius: 50%; }
.dot.red { background: #fb7185; } .dot.yellow { background: #fbbf24; } .dot.green { background: #34d399; }
.terminal-box pre { margin: 0; white-space: pre-wrap; word-break: break-all; }
.terminal-box code { font-family: 'JetBrains Mono'; color: #38bdf8; font-size: 13px; line-height: 1.6; }
.copy-btn-mini { position: absolute; bottom: 15px; right: 15px; background: rgba(255,255,255,0.05); border: 1px solid rgba(255,255,255,0.1); color: white; font-size: 11px; font-weight: 800; border-radius: 8px; }
.copy-btn-mini:hover { background: #7c3aed; border-color: #7c3aed; }

/* Configuration Modal */
.form-grid { display: flex; gap: 30px; }
.form-aside { width: 140px; }
.form-main { flex: 1; }

.dialog-inner { padding: 10px 5px; }

/* 深度优化输入框 visibility */
:deep(.el-input__wrapper) {
  background-color: #f8fafc !important;
  border: 1px solid #e2e8f0 !important;
  box-shadow: none !important;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

:deep(.el-input__wrapper.is-focus) {
  border-color: #7c3aed !important;
  background-color: #ffffff !important;
  box-shadow: 0 0 0 3px rgba(124, 58, 237, 0.1) !important;
}

:deep(.el-input__inner) {
  font-family: 'JetBrains Mono', monospace;
  font-weight: 600;
  color: #1e1b4b;
}

.protocol-selector-v2 { display: flex; flex-direction: column; gap: 10px; padding-right: 15px; border-right: 1px solid #f1f5f9; }
.p-item { padding: 12px 16px; border-radius: 12px; background: #f8fafc; color: #64748b; font-weight: 800; font-size: 12px; cursor: pointer; transition: all 0.2s; border: 1px solid transparent; text-align: center; }
.p-item:hover { background: rgba(124, 58, 237, 0.05); color: #7c3aed; }
.p-item.active { background: #7c3aed; color: white; box-shadow: 0 4px 12px rgba(124, 58, 237, 0.2); }

.advanced-section { 
  padding: 24px; 
  margin-top: 20px; 
  background: rgba(124, 58, 237, 0.03);
  border: 1px solid rgba(124, 58, 237, 0.1); 
  border-radius: 20px;
}
.section-label { display: block; font-size: 11px; font-weight: 900; color: #7c3aed; text-transform: uppercase; margin-bottom: 20px; letter-spacing: 0.8px; }

.random-btn {
  background: #7c3aed !important;
  color: white !important;
  border: none !important;
  font-weight: 800;
  font-size: 13px;
  height: 42px; /* Fixed height to match input */
  padding: 0 24px !important;
  margin: 0 !important;
  border-radius: 0 12px 12px 0 !important;
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 100px; /* Ensure it's wide enough */
}

/* 解决 append 容器与按钮的完美贴合 */
:deep(.vkey-input .el-input-group__append) {
  background-color: #7c3aed !important;
  border: none !important;
  padding: 0 !important;
  overflow: hidden;
  border-radius: 0 12px 12px 0;
  box-shadow: none !important;
}

:deep(.vkey-input .el-input__wrapper) {
  border-radius: 12px 0 0 12px !important;
  height: 42px;
}

.dialog-footer-v2 { display: flex; justify-content: space-between; align-items: center; width: 100%; border-top: 1px solid #f1f5f9; padding-top: 20px; }
.warning-text { font-size: 12px; color: #94a3b8; font-weight: 600; }
.purple-btn { background: #7c3aed !important; border: none !important; color: white !important; font-weight: 800; border-radius: 10px; padding: 0 25px; height: 42px; }
</style>
