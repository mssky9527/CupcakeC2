<template>
  <div class="module-panel">
    <div class="panel-head">
      <div>
        <h3>重能力 / 隔离宿主</h3>
        <p class="hint">
          终端 / 文件 / 进程已内置。日常只需推送 <code>iso_host</code>（隔离执行 BOF/.NET）。
          推送成功且仍存活时，「推送到本机」会置灰。
        </p>
      </div>
      <div class="head-actions">
        <el-button :loading="listing" @click="listOnAgent">刷新存活状态</el-button>
        <el-button :loading="loading" @click="refresh">刷新仓库</el-button>
      </div>
    </div>

    <el-alert
      v-if="!modules.length"
      type="warning"
      show-icon
      :closable="false"
      title="仓库为空：请先在「模块」页上传 iso_host（cupcake-iso-host.exe）"
      class="mb"
    />

    <el-alert
      v-if="aliveSummary"
      type="success"
      show-icon
      :closable="false"
      :title="aliveSummary"
      class="mb"
    />
    <el-alert
      v-else-if="listedOnce"
      type="info"
      show-icon
      :closable="false"
      title="当前主机未检测到已加载模块（可推送 iso_host）"
      class="mb"
    />

    <el-table :data="displayModules" class="mt" v-loading="loading" empty-text="无已登记模块">
      <el-table-column prop="id" label="ID" width="120" />
      <el-table-column prop="name" label="名称" width="150" />
      <el-table-column prop="description" label="描述" min-width="240" show-overflow-tooltip />
      <el-table-column label="大小" width="100">
        <template #default="{ row }">{{ formatSize(row.size) }}</template>
      </el-table-column>
      <el-table-column label="本机状态" width="130">
        <template #default="{ row }">
          <el-tag v-if="isAlive(row)" type="success" size="small" effect="dark">已推送·存活</el-tag>
          <el-tag v-else type="info" size="small">未推送</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="160" fixed="right">
        <template #default="{ row }">
          <el-button
            size="small"
            type="primary"
            :loading="pushing === row.id"
            :disabled="isAlive(row)"
            @click="pushModule(row)"
          >
            {{ isAlive(row) ? '已在本机' : '推送到本机' }}
          </el-button>
        </template>
      </el-table-column>
    </el-table>

    <p class="foot-note">
      推荐只关注 <code>iso_host</code>。bof / dotnet / shell 为遗留或实验项，可折叠隐藏。
      <el-switch v-model="showLegacy" active-text="显示遗留模块" style="margin-left: 12px" />
    </p>
  </div>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { ElMessage, ElNotification } from 'element-plus'
import api from '../../api/index'

const props = defineProps({
  clientId: { type: String, required: true },
  clientInfo: { type: Object, default: null },
  socket: { type: Object, default: null }
})

const loading = ref(false)
const pushing = ref('')
const listing = ref(false)
const modules = ref([])
const listedOnce = ref(false)
const showLegacy = ref(false)

const displayModules = computed(() => {
  if (showLegacy.value) return modules.value
  // Default: only show host (iso_host) + anything already alive
  return modules.value.filter(
    (m) => m.kind === 'host' || m.id === 'iso_host' || isAlive(m)
  )
})

const aliveSummary = computed(() => {
  const alive = modules.value.filter((m) => isAlive(m)).map((m) => m.name || m.id)
  if (!alive.length) return ''
  return `本机已存活：${alive.join('、')}`
})

const formatSize = (n) => {
  if (!n) return '—'
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(2)} MB`
}

const isAlive = (row) => !!(row && (row.loaded_on_agent || row.alive))

const normalizeList = (list) =>
  (list || []).map((m) =>
    typeof m === 'string'
      ? { id: m, name: m, description: '', size: 0, kind: 'custom', loaded_on_agent: false }
      : { ...m, loaded_on_agent: !!m.loaded_on_agent }
  )

const refresh = async () => {
  loading.value = true
  try {
    const res = await api.get('/api/modules', { params: { uuid: props.clientId } })
    modules.value = normalizeList(res.data?.modules)
  } catch (e) {
    ElMessage.error(e?.response?.data?.error || '加载失败')
  } finally {
    loading.value = false
  }
}

const pushModule = async (row) => {
  const id = row.id
  if (isAlive(row)) {
    ElMessage.info(`模块「${row.name || id}」已在本机存活，无需重复推送`)
    return
  }
  pushing.value = id
  try {
    const res = await api.post('/api/modules/push', { uuid: props.clientId, id })
    const data = res.data || {}
    row.loaded_on_agent = true
    row.alive = true
    ElNotification({
      title: '推送成功',
      message: data.msg || `模块 ${data.name || id} 已在目标主机就绪`,
      type: 'success',
      duration: 4500
    })
    if (data.warning) ElMessage.warning(data.warning)
    await refresh()
  } catch (e) {
    ElNotification({
      title: '推送失败',
      message: e?.response?.data?.error || '推送失败',
      type: 'error',
      duration: 5000
    })
  } finally {
    pushing.value = ''
  }
}

const listOnAgent = async () => {
  listing.value = true
  try {
    const res = await api.post('/api/modules/query', { uuid: props.clientId })
    listedOnce.value = true
    // Prefer catalog with loaded flags; do NOT dump raw JSON to the page
    if (Array.isArray(res.data?.modules)) {
      modules.value = normalizeList(res.data.modules)
    } else {
      await refresh()
    }
    // result is comma-separated ids from agent (may be empty)
    const raw = (res.data?.result || '').trim()
    if (raw) {
      const ids = raw.split(',').map((s) => s.trim()).filter(Boolean)
      for (const m of modules.value) {
        if (ids.includes(m.id)) {
          m.loaded_on_agent = true
          m.alive = true
        }
      }
      ElMessage.success(`已同步：Agent 报告已加载 ${ids.join(', ')}`)
    } else {
      ElMessage.success('已同步：Agent 当前无已加载模块')
    }
  } catch (e) {
    ElMessage.error(e?.response?.data?.error || '查询失败')
  } finally {
    listing.value = false
  }
}

onMounted(async () => {
  await refresh()
  try {
    await listOnAgent()
  } catch (_) {
    /* optional */
  }
})
</script>

<style scoped>
.module-panel { padding: 16px 20px; }
.panel-head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 16px;
  gap: 12px;
}
.head-actions { display: flex; gap: 8px; flex-shrink: 0; }
.hint { margin: 8px 0 0; opacity: 0.75; line-height: 1.5; max-width: 640px; }
.mt { margin-top: 16px; }
.mb { margin-bottom: 12px; }
.foot-note {
  margin-top: 14px;
  font-size: 12px;
  opacity: 0.65;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}
</style>
