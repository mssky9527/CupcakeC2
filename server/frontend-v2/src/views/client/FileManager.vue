<template>
  <div class="file-manager" v-loading="loading" element-loading-text="正在读取远程文件...">
    <div class="toolbar-container">
      <div class="nav-buttons">
        <el-button-group>
          <el-button :icon="Back" @click="goUp" :disabled="isRoot" title="返回上级" />
          <el-button :icon="Refresh" @click="refresh" title="刷新" />
        </el-button-group>
      </div>

      <el-input 
        v-model="inputPath" 
        class="address-bar" 
        placeholder="输入路径 (e.g. C:\Windows\)"
        @keyup.enter="navigateTo(inputPath)"
      >
        <template #prefix>
          <el-icon><Monitor /></el-icon>
        </template>
        <template #append>
          <el-button :icon="Right" @click="navigateTo(inputPath)" />
        </template>
      </el-input>

      <div class="action-buttons">
        <transition name="el-fade-in">
          <el-button v-if="selection.length > 0" type="danger" :icon="Delete" plain @click="handleBatchDelete">批量删除 ({{ selection.length }})</el-button>
        </transition>
      </div>
    </div>

      <div class="file-list-container" @contextmenu.prevent="handleRightClick">
        <el-table 
          :data="files" 
          style="width: 100%; height: 100%;" 
          @row-dblclick="handleDoubleClick"
          @selection-change="handleSelectionChange"
          height="100%"
          :row-style="{ cursor: 'pointer' }"
          size="small"
          empty-text="目录为空或读取失败"
        >
          <el-table-column type="selection" width="55" align="center" />
          <el-table-column width="50" align="center">
            <template #default="scope">
              <div style="display: flex; align-items: center; justify-content: center; height: 100%;">
                <el-icon v-if="scope.row.is_dir" size="20" color="#E6A23C"><Folder /></el-icon>
                <el-icon v-else size="20" color="#909399"><Document /></el-icon>
              </div>
            </template>
          </el-table-column>

        <el-table-column prop="name" label="名称" min-width="300" sortable show-overflow-tooltip>
          <template #default="scope">
            <span style="font-weight: 500;">{{ scope.row.name }}</span>
          </template>
        </el-table-column>

        <el-table-column prop="mod_time" label="修改日期" width="180" sortable>
            <template #default="scope">
                {{ formatTime(scope.row.mod_time) }}
            </template>
        </el-table-column>

        <el-table-column prop="size" label="大小" width="120" sortable align="right">
          <template #default="scope">
            {{ scope.row.is_dir ? '-' : formatSize(scope.row.size) }}
          </template>
        </el-table-column>

          <el-table-column label="操作" width="100" align="center">
            <template #default="scope">
              <el-dropdown trigger="click" @command="(cmd) => handleCommand(cmd, scope.row)">
                <div style="display: flex; align-items: center; justify-content: center; height: 100%; cursor: pointer;">
                  <el-icon color="#409EFF" size="18"><MoreFilled /></el-icon>
                </div>
                <template #dropdown>
                  <el-dropdown-menu>
                    <el-dropdown-item v-if="!scope.row.is_dir" command="preview" :icon="View">预览</el-dropdown-item>
                    <el-dropdown-item v-if="!scope.row.is_dir" command="download" :icon="Download">下载</el-dropdown-item>
                    <el-dropdown-item command="delete" :icon="Delete" style="color: #F56C6C;">删除</el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
            </template>
          </el-table-column>
        </el-table>

        <!-- Custom Context Menu for Directory -->
        <div v-show="contextMenuVisible" :style="{left: menuLeft + 'px', top: menuTop + 'px'}" class="context-menu">
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
      <span v-if="currentPath" style="margin-left: 20px;">当前: {{ currentPath }}</span>
    </div>

    <!-- Hidden Native File Input (500MB max for safety) -->
    <input type="file" ref="fileInputRef" style="display: none" @change="processUpload" />

    <!-- File Preview Dialog -->
    <el-dialog v-model="previewVisible" title="文件预览" width="60%" destroy-on-close>
      <pre class="code-preview">{{ previewContent }}</pre>
      <template #footer>
        <el-button @click="previewVisible = false">关闭</el-button>
      </template>
    </el-dialog>

    <!-- Transfer Progress Dialog -->
    <el-dialog v-model="transferVisible" :title="transferTitle" width="400px" :close-on-click-modal="false" :show-close="false" destroy-on-close align-center>
      <div style="text-align: center; padding: 20px 0;">
         <el-progress type="circle" :percentage="transferProgress" :status="transferStatus" :stroke-width="10" />
         <div style="margin-top: 20px; color: #606266; font-size: 14px;">{{ transferProgress === 100 ? '正在处理后端响应...' : '正在传输数据（分块中），请稍候...' }}</div>
      </div>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted, computed, reactive, watch } from 'vue'
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
    const res = await listFiles({ uuid: props.clientId, path: path })
    if (res.data) {
      files.value = res.data.files || []
      if (res.data.current_path) {
        currentPath.value = res.data.current_path
        inputPath.value = res.data.current_path
      }
    }
  } catch (err) {
    ElMessage.error('读取目录失败')
  } finally {
    loading.value = false
  }
}

const handleDoubleClick = (row) => {
  if (row.is_dir) {
    const sep = currentPath.value.includes('/') ? '/' : '\\'
    let nextPath = currentPath.value
    if (nextPath !== '.' && !nextPath.endsWith(sep)) nextPath += sep
    if (nextPath === '.') nextPath = ''
    nextPath += row.name
    loadFiles(nextPath)
  }
}

const goUp = () => {
  const sep = currentPath.value.includes('/') ? '/' : '\\'
  let target = currentPath.value + sep + '..'
  loadFiles(target)
}

const navigateTo = (path) => loadFiles(path)
const refresh = () => loadFiles(currentPath.value)

const handleRightClick = (event) => {
  contextMenuVisible.value = true
  menuLeft.value = event.clientX
  menuTop.value = event.clientY
}

// Close context menu on global click
const closeMenu = () => { contextMenuVisible.value = false }

onMounted(() => {
  window.addEventListener('click', closeMenu)
})

onUnmounted(() => {
  window.removeEventListener('click', closeMenu)
})

const triggerUpload = () => {
  if (fileInputRef.value) fileInputRef.value.click()
}

const processUpload = async (e) => {
  const file = e.target.files[0]
  if (!file) return

  // 🛡️ 大小限制：单文件最大 500MB（分块传输虽然安全，但过大的文件会导致浏览器上传超时）
  const MAX_FILE_SIZE = 500 * 1024 * 1024
  if (file.size > MAX_FILE_SIZE) {
    ElMessage.error(`文件过大（${(file.size / 1024 / 1024).toFixed(1)} MB），请选择小于 500MB 的文件`)
    e.target.value = ''
    return
  }

  loading.value = true
  ElMessage.info('正在准备上传...')
  
  try {
    const formData = new FormData()
    formData.append('uuid', props.clientId)
    const sep = currentPath.value.includes('/') ? '/' : '\\'
    let base = currentPath.value
    if (base !== '.' && !base.endsWith(sep)) base += sep
    const targetPath = (base === '.' ? '' : base) + file.name
    
    formData.append('path', targetPath)
    formData.append('file', file)

    transferTitle.value = `上传: ${file.name}`
    transferProgress.value = 0
    transferStatus.value = ''
    transferVisible.value = true

    await uploadFile(formData, (progressEvent) => {
      const percentCompleted = Math.round((progressEvent.loaded * 100) / progressEvent.total)
      transferProgress.value = percentCompleted
    })
    
    transferStatus.value = 'success'
    setTimeout(() => {
      transferVisible.value = false
      ElMessage.success('上传成功')
      refresh()
    }, 800)
  } catch(e) {
    transferStatus.value = 'exception'
    setTimeout(() => transferVisible.value = false, 1500)
    ElMessage.error('上传失败')
  } finally {
    loading.value = false
    e.target.value = '' // Reset input
  }
}

const downloadFile = async (row) => {
  if (row.is_dir) return
  ElMessage.info('开始请求下载组件...')
  try {
    const fullPath = currentPath.value + (currentPath.value.includes('/') ? '/' : '\\') + row.name
    
    transferTitle.value = `下载: ${row.name}`
    transferProgress.value = 0
    transferStatus.value = ''
    transferVisible.value = true

    const response = await fsDownload({ uuid: props.clientId, path: fullPath }, (progressEvent) => {
      // Chunked 传输可能没有 total (长度未知)
      if (progressEvent.total) {
        const percentCompleted = Math.round((progressEvent.loaded * 100) / progressEvent.total)
        transferProgress.value = percentCompleted
      } else if (row.size) {
        // Fallback: estimate from directory listing size
        let percent = Math.round((progressEvent.loaded * 100) / row.size)
        transferProgress.value = percent > 100 ? 100 : percent
      }
    })
    
    // Ensure it shows 100% on completion if no total was provided
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
    setTimeout(() => transferVisible.value = false, 1500)
    ElMessage.error('下载失败')
  }
}

const deletePath = async (row) => {
  try {
    await ElMessageBox.confirm(`确定要删除 ${row.is_dir ? '文件夹' : '文件'}: ${row.name} 吗?`, '警告', {
      type: 'warning',
      confirmButtonText: '确定',
      cancelButtonText: '取消'
    })
    
    const fullPath = getFullPath(row.name)
    await deleteFiles({ uuid: props.clientId, paths: [fullPath] })
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

const getFullPath = (name) => {
  const sep = currentPath.value.includes('/') ? '/' : '\\'
  let base = currentPath.value
  if (base === '.') return name
  if (!base.endsWith(sep)) base += sep
  return base + name
}

const handlePreview = async (row) => {
  const isText = /\.(txt|log|conf|ini|cfg|sh|bat|ps1|php|jsp|asp|html|js|css|py|go|c|cpp|h|json|xml|yaml|yml|md)$/i.test(row.name)
  
  if (!isText && row.size > 1024 * 10) {
     try {
       await ElMessageBox.confirm('该文件可能不是纯文本且体积较大，预览可能产生乱码。确定要预览吗？', '提示', {
         confirmButtonText: '确定',
         cancelButtonText: '取消',
         type: 'warning'
       })
     } catch (e) { return }
  }

  loading.value = true
  try {
    const fullPath = getFullPath(row.name)
    const res = await readFile({ uuid: props.clientId, path: fullPath })
    if (res.data && res.data.content) {
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
    await ElMessageBox.confirm(`确定要删除选中的 ${selection.value.length} 个项目吗?`, '警告', {
      type: 'warning',
      confirmButtonText: '确定',
      cancelButtonText: '取消'
    })
    
    const paths = selection.value.map(f => getFullPath(f.name))
    loading.value = true
    const res = await deleteFiles({ uuid: props.clientId, paths })
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
    } catch(e) {}
}

defineExpose({ handleSocketMessage })

onMounted(() => {
  loadFiles('.') 
})
</script>

<style scoped>
.file-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: #fff;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  overflow: hidden;
}

.toolbar-container {
  padding: 10px 15px;
  display: flex;
  gap: 12px;
  border-bottom: 1px solid #ebeef5;
  background-color: #f8f9fa;
  align-items: center;
}

.address-bar {
  flex: 1;
}

.file-list-container {
  flex: 1;
  overflow: hidden;
}

.status-bar {
  padding: 8px 15px;
  font-size: 13px;
  color: #909399;
  background: #fcfcfc;
  border-top: 1px solid #ebeef5;
  display: flex;
  align-items: center;
}

.code-preview {
  background: #282c34;
  color: #abb2bf;
  padding: 15px;
  border-radius: 4px;
  overflow: auto;
  max-height: 500px;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  font-size: 13px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
}

.context-menu {
  position: fixed;
  background: white;
  border: 1px solid #ebeef5;
  box-shadow: 0 4px 12px rgba(0,0,0,0.1);
  z-index: 9999;
  border-radius: 4px;
  padding: 5px 0;
  min-width: 180px;
}

.menu-item {
  padding: 10px 16px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: #606266;
  transition: all 0.2s;
}

.menu-item:hover {
  background-color: #f5f7fa;
  color: #409EFF;
}

.menu-item .el-icon {
  font-size: 14px;
}
</style>
