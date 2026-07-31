<template>
  <div class="file-manager" v-loading="loading" element-loading-text="正在读取远程文件...">
    <div class="toolbar-container">
      <el-button-group>
        <el-button :icon="Back" @click="goUp" :disabled="isRoot" title="返回上级" />
        <el-button :icon="Refresh" @click="refresh" title="刷新" />
      </el-button-group>

      <el-input
        v-model="inputPath"
        class="address-bar"
        placeholder="输入路径，例如 C:\Windows\"
        @keyup.enter="navigateTo(inputPath)"
      >
        <template #prefix>
          <el-icon><Monitor /></el-icon>
        </template>
        <template #append>
          <el-button :icon="Right" @click="navigateTo(inputPath)" />
        </template>
      </el-input>

      <el-button v-if="selection.length > 0" type="danger" :icon="Delete" plain @click="handleBatchDelete">
        批量删除 ({{ selection.length }})
      </el-button>
    </div>

    <div class="file-list-container" @contextmenu.prevent="handleRightClick">
      <el-table
        :data="files"
        height="100%"
        size="small"
        empty-text="目录为空或读取失败"
        :row-style="{ cursor: 'pointer' }"
        @row-dblclick="handleDoubleClick"
        @selection-change="handleSelectionChange"
      >
        <el-table-column type="selection" width="55" align="center" />
        <el-table-column width="50" align="center">
          <template #default="scope">
            <el-icon v-if="scope.row.is_dir" size="20" color="#111111"><Folder /></el-icon>
            <el-icon v-else size="20" color="#777777"><Document /></el-icon>
          </template>
        </el-table-column>
        <el-table-column prop="name" label="名称" min-width="300" sortable show-overflow-tooltip />
        <el-table-column prop="mod_time" label="修改日期" width="180" sortable>
          <template #default="scope">{{ formatTime(scope.row.mod_time) }}</template>
        </el-table-column>
        <el-table-column prop="size" label="大小" width="120" sortable align="right">
          <template #default="scope">{{ scope.row.is_dir ? '-' : formatSize(scope.row.size) }}</template>
        </el-table-column>
        <el-table-column label="操作" width="100" align="center">
          <template #default="scope">
            <el-dropdown trigger="click" @command="(cmd) => handleCommand(cmd, scope.row)">
              <el-icon color="#111111" size="18"><MoreFilled /></el-icon>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item v-if="!scope.row.is_dir" command="preview" :icon="View">预览</el-dropdown-item>
                  <el-dropdown-item v-if="!scope.row.is_dir" command="download" :icon="Download">下载</el-dropdown-item>
                  <el-dropdown-item command="delete" :icon="Delete">删除</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </template>
        </el-table-column>
      </el-table>

      <div v-show="contextMenuVisible" :style="{ left: menuLeft + 'px', top: menuTop + 'px' }" class="context-menu">
        <div class="menu-item" @click="triggerUpload">
          <el-icon><Upload /></el-icon> 上传文件到当前目录
        </div>
        <div class="menu-item" @click="refresh">
          <el-icon><Refresh /></el-icon> 刷新
        </div>
      </div>
    </div>

    <div class="status-bar">
      <span>{{ files.length }} 个项目</span>
      <span v-if="currentPath" class="current-path">当前: {{ currentPath }}</span>
    </div>

    <input type="file" ref="fileInputRef" class="hidden-input" @change="processUpload" />

    <el-dialog v-model="previewVisible" title="文件预览" width="60%" destroy-on-close>
      <pre class="code-preview">{{ previewContent }}</pre>
      <template #footer>
        <el-button @click="previewVisible = false">关闭</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="transferVisible" :title="transferTitle" width="400px" :close-on-click-modal="false" :show-close="false" destroy-on-close align-center>
      <div class="transfer-box">
        <el-progress type="circle" :percentage="transferProgress" :status="transferStatus" :stroke-width="10" />
        <div class="transfer-text">
          {{ transferProgress === 100 ? '正在处理后端响应...' : '正在传输数据，请稍候...' }}
        </div>
      </div>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted, computed } from 'vue'
import { listFiles, readFile, deleteFiles, uploadFile } from '@/api/file'
import { fsDownload } from '@/api/index'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Folder, Document, Back, Refresh, Right, Upload, Monitor, MoreFilled, Download, Delete, View } from '@element-plus/icons-vue'

const props = defineProps({
  clientId: { type: String, required: true },
  socket: { type: Object, default: null }
})

const files = ref([])
const currentPath = ref('')
const inputPath = ref('')
const loading = ref(false)
const fileInputRef = ref(null)
const selection = ref([])
const previewVisible = ref(false)
const previewContent = ref('')
const contextMenuVisible = ref(false)
const menuLeft = ref(0)
const menuTop = ref(0)
const transferVisible = ref(false)
const transferProgress = ref(0)
const transferTitle = ref('')
const transferStatus = ref('')

const isRoot = computed(() => {
  if (!currentPath.value) return true
  return currentPath.value === '/' || currentPath.value === '.' || currentPath.value.endsWith(':\\') || currentPath.value.endsWith(':/')
})

const formatTime = (ts) => {
  if (!ts) return '-'
  const d = new Date(ts > 1e11 ? ts : ts * 1000)
  return d.toLocaleString()
}

const formatSize = (bytes) => {
  if (!bytes || bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i]
}

const loadFiles = async (path) => {
  loading.value = true
  try {
    const res = await listFiles({ uuid: props.clientId, path })
    files.value = res.data?.files || []
    currentPath.value = res.data?.current_path || path
    inputPath.value = currentPath.value
  } catch (err) {
    ElMessage.error('读取目录失败')
  } finally {
    loading.value = false
  }
}

const handleDoubleClick = (row) => {
  if (!row.is_dir) return
  const sep = currentPath.value.includes('/') ? '/' : '\\'
  let nextPath = currentPath.value
  if (nextPath !== '.' && !nextPath.endsWith(sep)) nextPath += sep
  if (nextPath === '.') nextPath = ''
  loadFiles(nextPath + row.name)
}

const goUp = () => {
  const sep = currentPath.value.includes('/') ? '/' : '\\'
  loadFiles(currentPath.value + sep + '..')
}

const navigateTo = (path) => loadFiles(path)
const refresh = () => loadFiles(currentPath.value)

const handleRightClick = (event) => {
  contextMenuVisible.value = true
  menuLeft.value = event.clientX
  menuTop.value = event.clientY
}

const closeMenu = () => { contextMenuVisible.value = false }

const triggerUpload = () => {
  fileInputRef.value?.click()
}

const getFullPath = (name) => {
  const sep = currentPath.value.includes('/') ? '/' : '\\'
  let base = currentPath.value
  if (base === '.') return name
  if (!base.endsWith(sep)) base += sep
  return base + name
}

const processUpload = async (event) => {
  const file = event.target.files[0]
  if (!file) return

  const maxFileSize = 500 * 1024 * 1024
  if (file.size > maxFileSize) {
    ElMessage.error(`文件过大: ${(file.size / 1024 / 1024).toFixed(1)} MB，请选择小于 500MB 的文件`)
    event.target.value = ''
    return
  }

  loading.value = true
  ElMessage.info('正在准备上传...')

  try {
    const formData = new FormData()
    formData.append('uuid', props.clientId)
    formData.append('path', getFullPath(file.name))
    formData.append('file', file)

    transferTitle.value = '上传: ' + file.name
    transferProgress.value = 0
    transferStatus.value = ''
    transferVisible.value = true

    await uploadFile(formData, (progressEvent) => {
      transferProgress.value = Math.round((progressEvent.loaded * 100) / progressEvent.total)
    })

    transferStatus.value = 'success'
    setTimeout(() => {
      transferVisible.value = false
      ElMessage.success('上传成功')
      refresh()
    }, 800)
  } catch (e) {
    transferStatus.value = 'exception'
    setTimeout(() => { transferVisible.value = false }, 1500)
    const detail =
      e?.response?.data?.error ||
      e?.response?.data?.msg ||
      e?.message ||
      '未知错误'
    ElMessage.error('上传失败: ' + detail)
    console.error('[FileManager] upload failed', e)
  } finally {
    loading.value = false
    event.target.value = ''
  }
}

const downloadFile = async (row) => {
  if (row.is_dir) return
  ElMessage.info('开始请求下载组件...')

  try {
    transferTitle.value = '下载: ' + row.name
    transferProgress.value = 0
    transferStatus.value = ''
    transferVisible.value = true

    const response = await fsDownload({ uuid: props.clientId, path: getFullPath(row.name) }, (progressEvent) => {
      if (progressEvent.total) {
        transferProgress.value = Math.round((progressEvent.loaded * 100) / progressEvent.total)
      } else if (row.size) {
        transferProgress.value = Math.min(100, Math.round((progressEvent.loaded * 100) / row.size))
      }
    })

    transferProgress.value = 100
    transferStatus.value = 'success'

    const url = window.URL.createObjectURL(new Blob([response.data]))
    const link = document.createElement('a')
    link.href = url
    link.setAttribute('download', row.name)
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)

    setTimeout(() => { transferVisible.value = false }, 800)
    ElMessage.success('下载完成')
  } catch (e) {
    transferStatus.value = 'exception'
    setTimeout(() => { transferVisible.value = false }, 1500)
    ElMessage.error('下载失败')
  }
}

const deletePath = async (row) => {
  try {
    await ElMessageBox.confirm(`确定要删除${row.is_dir ? '文件夹' : '文件'}: ${row.name} 吗？`, '警告', {
      type: 'warning',
      confirmButtonText: '确认删除',
      cancelButtonText: '取消'
    })

    await deleteFiles({ uuid: props.clientId, paths: [getFullPath(row.name)] })
    ElMessage.success('删除指令已发送')
    setTimeout(refresh, 500)
  } catch (e) {}
}

const handleCommand = (cmd, row) => {
  if (cmd === 'preview') handlePreview(row)
  if (cmd === 'download') downloadFile(row)
  if (cmd === 'delete') deletePath(row)
}

const handleSelectionChange = (val) => {
  selection.value = val
}

const handlePreview = async (row) => {
  const isText = /\.(txt|log|conf|ini|cfg|sh|bat|ps1|php|jsp|asp|html|js|css|py|go|c|cpp|h|json|xml|yaml|yml|md)$/i.test(row.name)

  if (!isText && row.size > 1024 * 10) {
    try {
      await ElMessageBox.confirm('该文件可能不是纯文本且体积较大，预览可能产生乱码。确定要预览吗？', '提示', {
        confirmButtonText: '继续预览',
        cancelButtonText: '取消',
        type: 'warning'
      })
    } catch (e) {
      return
    }
  }

  loading.value = true
  try {
    const res = await readFile({ uuid: props.clientId, path: getFullPath(row.name) })
    if (res.data?.content) {
      previewContent.value = res.data.content
      previewVisible.value = true
    } else {
      ElMessage.warning('文件内容为空或无法读取')
    }
  } catch (err) {
    ElMessage.error('读取文件内容失败: ' + (err.response?.data?.error || err.message))
  } finally {
    loading.value = false
  }
}

const handleBatchDelete = async () => {
  try {
    await ElMessageBox.confirm(`确定要删除选中的 ${selection.value.length} 个项目吗？`, '警告', {
      type: 'warning',
      confirmButtonText: '确认删除',
      cancelButtonText: '取消'
    })

    const paths = selection.value.map(f => getFullPath(f.name))
    loading.value = true
    await deleteFiles({ uuid: props.clientId, paths })
    ElMessage.success('批量删除指令已发送')
    setTimeout(refresh, 500)
  } catch (e) {
  } finally {
    loading.value = false
  }
}

const handleSocketMessage = (event) => {
  try {
    const data = JSON.parse(event.data)
    if (data.type === 'JSON_DATA') {
      const inner = JSON.parse(data.content)
      if (inner.current_path || inner.files) {
        files.value = inner.files || []
        currentPath.value = inner.current_path || currentPath.value
        inputPath.value = currentPath.value
        loading.value = false
      }
    }
  } catch (e) {}
}

defineExpose({ handleSocketMessage })

onMounted(() => {
  window.addEventListener('click', closeMenu)
  loadFiles('.')
})

onUnmounted(() => {
  window.removeEventListener('click', closeMenu)
})
</script>

<style scoped>
.file-manager {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.toolbar-container {
  display: flex;
  align-items: center;
  gap: 12px;
}

.address-bar {
  flex: 1;
}

.file-list-container {
  flex: 1;
  position: relative;
  min-height: 0;
}

.context-menu {
  position: fixed;
  z-index: 3000;
  min-width: 180px;
  padding: 6px;
  background: var(--bg-panel-strong);
  border: 1px solid var(--line-muted);
  border-radius: 8px;
  box-shadow: 0 12px 30px rgba(15, 23, 42, 0.12);
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 10px;
  cursor: pointer;
  border-radius: 6px;
  color: var(--text-strong);
}

.menu-item:hover {
  background: var(--surface-muted);
}

.status-bar {
  display: flex;
  align-items: center;
  color: var(--text-muted);
  font-size: 12px;
}

.current-path {
  margin-left: 20px;
}

.hidden-input {
  display: none;
}

.code-preview {
  max-height: 60vh;
  overflow: auto;
  white-space: pre-wrap;
  font-family: 'JetBrains Mono', monospace;
}

.transfer-box {
  text-align: center;
  padding: 20px 0;
}

.transfer-text {
  margin-top: 20px;
  color: #555555;
  font-size: 14px;
}
</style>
