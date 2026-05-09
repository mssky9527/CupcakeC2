<template>
  <div class="view-shell listener-shell">
    <section class="view-actions listener-actions">
      <el-button @click="fetchListeners">
        <el-icon><Refresh /></el-icon>
        刷新
      </el-button>
      <el-button type="primary" @click="openCreateDialog">
        <el-icon><Plus /></el-icon>
        新建监听器
      </el-button>
    </section>

    <section class="stat-grid">
      <article class="surface-card stat-card">
        <div class="stat-card__icon">
          <el-icon><Headset /></el-icon>
        </div>
        <div>
          <span class="stat-card__label">运行中</span>
          <div class="stat-card__value">{{ runningCount }}</div>
        </div>
      </article>

      <article class="surface-card stat-card">
        <div class="stat-card__icon">
          <el-icon><Connection /></el-icon>
        </div>
        <div>
          <span class="stat-card__label">监听总数</span>
          <div class="stat-card__value">{{ listeners.length }}</div>
        </div>
      </article>

      <article class="surface-card stat-card">
        <div class="stat-card__icon">
          <el-icon><Monitor /></el-icon>
        </div>
        <div>
          <span class="stat-card__label">协议类型</span>
          <div class="stat-card__value">{{ protocolKinds }}</div>
        </div>
      </article>
    </section>

    <section class="surface-card table-shell listener-table-card">
      <div class="panel-head">
        <div>
          <span class="panel-kicker">Inventory</span>
          <h3>监听器清单</h3>
        </div>
        <div class="chip">实时状态</div>
      </div>

      <el-table :data="listeners" class="premium-table" v-loading="loading">
        <el-table-column width="64" align="center">
          <template #default="{ row }">
            <div class="protocol-icon" :class="protocolClass(row.protocol)">
              <el-icon v-if="row.protocol === 'TCP'"><Share /></el-icon>
              <el-icon v-else-if="row.protocol === 'WebSocket'"><Connection /></el-icon>
              <el-icon v-else-if="row.protocol === 'DNS'"><Monitor /></el-icon>
              <el-icon v-else><Promotion /></el-icon>
            </div>
          </template>
        </el-table-column>

        <el-table-column prop="protocol" label="协议" width="140">
          <template #default="{ row }">
            <el-tag :type="getProtocolType(row.protocol)" effect="plain" round>
              {{ row.protocol }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column label="监听地址" min-width="190">
          <template #default="{ row }">
            <code class="mono addr-code">{{ row.bind_ip || '0.0.0.0' }}:{{ row.port }}</code>
          </template>
        </el-table-column>

        <el-table-column label="公开投递 Host" min-width="170">
          <template #default="{ row }">
            <span class="muted">{{ row.public_host || '--' }}</span>
          </template>
        </el-table-column>

        <el-table-column prop="note" label="备注" min-width="160" show-overflow-tooltip>
          <template #default="{ row }">
            <span class="muted">{{ row.note || '--' }}</span>
          </template>
        </el-table-column>

        <el-table-column label="状态" width="120" align="center">
          <template #default="{ row }">
            <div class="status-indicator" :class="row.status?.toLowerCase()">
              <span class="dot"></span>
              {{ row.status === 'Running' ? '运行中' : '已停止' }}
            </div>
          </template>
        </el-table-column>

        <el-table-column label="操作" width="320" align="center" fixed="right">
          <template #default="{ row }">
            <div class="table-actions">
              <el-button link class="action-link" @click="openStagerDialog(row)">Stager</el-button>
              <el-button
                v-if="row.status === 'Stopped' || row.status === 'Failed'"
                link
                class="action-link"
                @click="handleStart(row.id)"
              >
                启动
              </el-button>
              <el-button
                v-else
                link
                class="action-link"
                @click="handleStop(row.id)"
              >
                停止
              </el-button>
              <el-button link class="action-link action-link--danger" @click="handleDelete(row.id)">删除</el-button>
            </div>
          </template>
        </el-table-column>
      </el-table>
    </section>

    <el-dialog v-model="stagerVisible" title="快速上线命令" width="680px" class="premium-dialog">
      <div class="dialog-stack">
        <el-radio-group v-model="stagerPlatform" @change="fetchStager">
          <el-radio-button label="windows">Windows</el-radio-button>
          <el-radio-button label="linux">Linux</el-radio-button>
        </el-radio-group>

        <div class="stager-section" v-loading="stagerLoading">
          <h4 class="stager-title">{{ stagerPlatform === 'windows' ? 'Windows 上线命令' : 'Linux 上线命令' }}</h4>
          <p class="dialog-hint" v-if="stagerPlatform === 'windows'">目标执行后自动判断 x64/x86 架构，下载对应版本 Agent。</p>
          <p class="dialog-hint" v-else>在目标 Linux 主机执行，自动下载并运行 Agent。</p>
          <div class="terminal-box">
            <div class="terminal-box__dots">
              <span></span>
              <span></span>
              <span></span>
            </div>
            <pre><code>{{ stagerCommand || '暂无可用命令' }}</code></pre>
          </div>
        </div>
      </div>

      <template #footer>
        <el-button @click="stagerVisible = false">关闭</el-button>
        <el-button type="primary" :disabled="!stagerCommand" @click="copyText(stagerCommand)">
          <el-icon><CopyDocument /></el-icon>
          复制命令
        </el-button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="dialogVisible"
      title="新建监听器"
      width="760px"
      class="premium-dialog"
      destroy-on-close
    >
      <el-form :model="form" label-position="top" class="listener-form">
        <div class="listener-form__layout">
          <div class="protocol-list">
            <button
              v-for="protocol in ['TCP', 'WebSocket', '正向TCP', 'DNS']"
              :key="protocol"
              type="button"
              class="protocol-list__item"
              :class="{ 'protocol-list__item--active': form.protocol === protocol }"
              @click="form.protocol = protocol; handleProtocolChange(protocol)"
            >
              {{ protocol }}
            </button>
          </div>

          <div class="listener-form__main">
            <div class="listener-form__grid">
              <el-form-item :label="form.protocol === '正向TCP' ? '监听端口' : '监听地址 (IP:Port)'">
                <el-input v-model="listenAddr" :placeholder="form.protocol === '正向TCP' ? '4444' : '0.0.0.0:8081'" />
              </el-form-item>

              <el-form-item label="备注">
                <el-input v-model="form.note" placeholder="例如：办公网 WebSocket 出口" />
              </el-form-item>
            </div>

            <el-form-item v-if="form.protocol !== '正向TCP'" label="公开投递 Host">
              <el-input v-model="form.public_host" placeholder="c2.example.com" />
            </el-form-item>

            <el-form-item v-if="form.protocol === 'DNS'" label="NS 域名委派" required>
              <el-input v-model="form.ns_domain" placeholder="ns1.corp.example" />
            </el-form-item>

            <div class="security-box">
              <span class="security-box__label">安全参数</span>

              <el-form-item label="通信密钥">
                <el-input v-model="form.encrypt_key" :type="showKey ? 'text' : 'password'" class="vkey-input">
                  <template #append>
                    <el-button @click="generateKey">随机生成</el-button>
                  </template>
                </el-input>
              </el-form-item>

              <div class="listener-form__grid">
                <el-form-item label="加密盐值">
                  <el-input v-model="form.encryption_salt" />
                </el-form-item>

                <el-form-item label="报文编码方式">
                  <el-select v-model="form.obfuscate_mode">
                    <el-option label="None" value="None" />
                    <el-option label="Base64" value="Base64" />
                    <el-option label="XOR Stream" value="XOR" />
                  </el-select>
                </el-form-item>
              </div>
            </div>
          </div>
        </div>
      </el-form>

      <template #footer>
        <div class="dialog-footer">
          <span class="muted">请确保目标端口已放行，并与投递方式保持一致。</span>
          <div class="dialog-footer__actions">
            <el-button @click="dialogVisible = false">取消</el-button>
            <el-button type="primary" :loading="submitting" @click="createListener">创建监听器</el-button>
          </div>
        </div>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, onMounted, reactive, ref } from 'vue'
import api from '../api/index'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Connection,
  CopyDocument,
  Headset,
  Monitor,
  Plus,
  Promotion,
  Refresh,
  Share
} from '@element-plus/icons-vue'

const listeners = ref([])
const loading = ref(false)
const dialogVisible = ref(false)
const submitting = ref(false)
const showKey = ref(false)
const listenAddr = ref('0.0.0.0:8081')

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

const runningCount = computed(() => listeners.value.filter((listener) => listener.status === 'Running').length)
const protocolKinds = computed(() => new Set(listeners.value.map((listener) => listener.protocol)).size || 0)

const fetchListeners = async () => {
  loading.value = true
  try {
    const res = await api.get('/api/listeners')
    listeners.value = res.data || []
  } catch {
    ElMessage.error('无法同步监听器列表')
  } finally {
    loading.value = false
  }
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
  for (let i = 0; i < 32; i += 1) {
    key += chars.charAt(Math.floor(Math.random() * chars.length))
  }
  form.encrypt_key = key
  showKey.value = true
}

const generateRandomSalt = () => {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
  let result = ''
  for (let i = 0; i < 6; i += 1) {
    result += chars.charAt(Math.floor(Math.random() * chars.length))
  }
  form.encryption_salt = result
}

const protocolClass = (protocol) => {
  const value = String(protocol || '').toLowerCase()
  if (value === 'tcp') return 'protocol-icon--tcp'
  if (value === 'websocket') return 'protocol-icon--websocket'
  if (value === 'dns') return 'protocol-icon--dns'
  return 'protocol-icon--bind'
}

const getProtocolType = (protocol) => {
  const map = { WebSocket: 'success', TCP: 'primary', 正向TCP: 'warning', DNS: 'warning' }
  return map[protocol] || 'info'
}

const createListener = async () => {
  const parts = listenAddr.value.split(':')
  if (parts.length === 2) {
    form.bind_ip = parts[0] || '0.0.0.0'
    form.port = parseInt(parts[1], 10)
  } else if (!Number.isNaN(Number(listenAddr.value))) {
    form.bind_ip = '0.0.0.0'
    form.port = parseInt(listenAddr.value, 10)
  }

  submitting.value = true
  try {
    await api.post('/api/listeners', { ...form })
    ElMessage.success('监听器创建成功')
    dialogVisible.value = false
    fetchListeners()
  } catch {
    ElMessage.error('监听器创建失败')
  } finally {
    submitting.value = false
  }
}

const handleStop = async (id) => {
  await api.post(`/api/listeners/${id}/stop`)
  ElMessage.warning('监听器已停止')
  fetchListeners()
}

const handleStart = async (id) => {
  await api.post(`/api/listeners/${id}/start`)
  ElMessage.success('监听器已启动')
  fetchListeners()
}

const handleDelete = (id) => {
  ElMessageBox.confirm('确认删除这个监听器吗？', '删除监听器', { type: 'warning' })
    .then(async () => {
      await api.delete(`/api/listeners/${id}`)
      ElMessage.success('监听器已删除')
      fetchListeners()
    })
    .catch(() => {})
}

const openStagerDialog = (row) => {
  currentListener.value = row
  stagerVisible.value = true
  fetchStager()
}

const fetchStager = async () => {
  if (!currentListener.value) return
  stagerLoading.value = true
  const host = currentListener.value.public_host || window.location.hostname
  try {
    const res = await api.get('/api/stager', {
      params: {
        listener_id: currentListener.value.id,
        os: stagerPlatform.value,
        arch: 'x64',
        host
      }
    })
    stagerCommand.value = res.data.command
  } catch {
    ElMessage.error('stager 生成失败')
  } finally {
    stagerLoading.value = false
  }
}

const copyText = async (text) => {
  if (!text) return
  await navigator.clipboard.writeText(text)
  ElMessage.success('命令已复制')
}

onMounted(fetchListeners)
</script>

<style scoped>
.listener-shell {
  gap: 20px;
}

.listener-actions {
  justify-content: flex-end;
}

.listener-table-card {
  padding-top: 20px;
}

.listener-table-card :deep(.el-table__header-wrapper th.el-table__cell),
.listener-table-card :deep(.el-table__body-wrapper td.el-table__cell) {
  white-space: nowrap;
}

.protocol-icon {
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
  border-radius: 12px;
  background: var(--surface-subtle);
  color: var(--text-strong);
}

.protocol-icon--tcp {
  background: #eff6ff;
  color: #2563eb;
}

.protocol-icon--websocket {
  background: #f0fdf4;
  color: #059669;
}

.protocol-icon--dns {
  background: #fff7ed;
  color: #d97706;
}

.protocol-icon--bind {
  background: #f5f3ff;
  color: #7c3aed;
}

.addr-code {
  padding: 4px 10px;
  border-radius: 999px;
  background: var(--surface-muted);
}

.status-indicator {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 4px 12px;
  border-radius: 999px;
  background: var(--surface-muted);
  color: var(--text-strong);
  font-size: 12px;
  font-weight: 700;
}

.status-indicator .dot {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: var(--text-strong);
}

.status-indicator.running {
  background: rgba(16, 185, 129, 0.12);
  color: #047857;
}

.status-indicator.running .dot {
  background: #10b981;
}

.table-actions {
  display: inline-flex;
  gap: 14px;
  flex-wrap: wrap;
  justify-content: center;
}

.action-link {
  font-weight: 700;
}

.action-link--danger {
  color: #b42318 !important;
}

.dialog-stack {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.dialog-hint {
  margin: 0 0 12px;
  color: var(--text-body);
  line-height: 1.6;
  font-size: 13px;
}

.terminal-box {
  padding: 18px;
  border-radius: 20px;
  background: #0f0f10;
}

.terminal-box__dots {
  display: flex;
  gap: 6px;
  margin-bottom: 12px;
}

.terminal-box__dots span {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.3);
}

.terminal-box pre {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-all;
}

.terminal-box code {
  color: #d9f99d;
  font-size: 12px;
  line-height: 1.7;
}

.stager-section {
  margin-bottom: 20px;
}

.stager-title {
  font-size: 15px;
  font-weight: 700;
  margin-bottom: 10px;
  color: var(--text-strong);
}

.copy-btn {
  margin-top: 8px;
}

.listener-form__layout {
  display: grid;
  grid-template-columns: 160px minmax(0, 1fr);
  gap: 24px;
}

.protocol-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.protocol-list__item {
  padding: 12px 14px;
  border: 1px solid var(--line-soft);
  border-radius: 16px;
  background: var(--surface-soft);
  color: var(--text-body);
  font-weight: 700;
  text-align: left;
  cursor: pointer;
}

.protocol-list__item--active {
  background: #ffffff;
  color: var(--text-strong);
  border-color: var(--text-strong);
}

.listener-form__main {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.listener-form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}

.security-box {
  padding: 18px;
  border-radius: 20px;
  background: var(--surface-soft);
  border: 1px solid var(--line-soft);
}

.security-box__label {
  display: inline-block;
  margin-bottom: 14px;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.14em;
  color: var(--text-muted);
}

.dialog-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  width: 100%;
}

.dialog-footer__actions {
  display: flex;
  gap: 10px;
}

@media (max-width: 900px) {
  .listener-actions {
    justify-content: flex-start;
  }

  .table-actions {
    gap: 10px;
    justify-content: flex-start;
  }

  .action-link {
    padding: 0;
    font-size: 12px;
  }

  .listener-form__layout,
  .listener-form__grid {
    grid-template-columns: 1fr;
  }

  .dialog-footer {
    flex-direction: column;
    align-items: stretch;
  }

  .dialog-footer__actions {
    width: 100%;
  }
}
</style>
