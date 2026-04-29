<template>
  <div class="plugin-management-container">
    <!-- Page Header (Bento style) -->
    <div class="page-header glass-panel mb-24">
      <div class="header-content">
        <div class="title-section">
          <h2 class="main-title">武器库 <span class="purple-text">插件中心</span></h2>
          <p class="sub-title">扩展指令与自动化套件 (Plugin & Extension Registry)</p>
        </div>
        <div class="header-actions">
           <el-input
              v-model="searchQuery"
              placeholder="搜索插件、功能或平台..."
              class="premium-search"
              prefix-icon="Search"
              clearable
            />
          <el-button class="premium-btn upload-btn" type="primary" :icon="Upload" @click="showUploadDialog = true">
            上传新插件 (UPLOAD)
          </el-button>
        </div>
      </div>
    </div>

    <!-- Stats Summary Row -->
    <div class="stats-row mb-24">
      <div class="stat-module glass-panel">
        <div class="stat-icon-box purple">
          <el-icon><Collection /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">已注册插件</div>
          <div class="stat-value">{{ plugins.length }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box blue">
          <el-icon><Platform /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">跨平台支持</div>
          <div class="stat-value">{{ Array.from(new Set(plugins.map(p => p.required_os))).length }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box orange">
          <el-icon><Cpu /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">内存载荷占比</div>
          <div class="stat-value">{{ plugins.filter(p => p.type.includes('mem') || p.type.includes('shellcode')).length }}</div>
        </div>
      </div>
    </div>

    <!-- Main Table Section -->
    <div class="table-module glass-panel">
      <el-table :data="filteredPlugins" v-loading="loading" class="premium-table">
        <el-table-column width="60" align="center">
          <template #default="{ row }">
             <div class="category-orb" :class="row.category || 'general'">
                <el-icon v-if="row.category === 'credentials'"><Lock /></el-icon>
                <el-icon v-else-if="row.category === 'lateral'"><Share /></el-icon>
                <el-icon v-else><Box /></el-icon>
             </div>
          </template>
        </el-table-column>

        <el-table-column label="插件名称与分类" min-width="200">
          <template #default="{ row }">
             <div class="name-cell">
                <span class="p-name">{{ row.name }}</span>
                <span class="p-category">{{ translateCategory(row.category) }}</span>
             </div>
          </template>
        </el-table-column>
        
        <el-table-column prop="description" label="核心功能描述" min-width="250" show-overflow-tooltip>
          <template #default="{ row }">
             <span class="desc-text">{{ row.description || '暂无详细功能描述' }}</span>
          </template>
        </el-table-column>
        
        <el-table-column label="运行环境" width="140" align="center">
          <template #default="{ row }">
            <el-tag 
              :type="getOsTag(row.required_os)" 
              class="premium-tag"
              effect="plain"
              round
            >
              {{ formatOS(row.required_os) }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column label="交互机制" width="200" align="center">
          <template #default="{ row }">
            <div class="type-capsule" :class="getTypeTag(row.type)">
               {{ translateType(row.type) }}
            </div>
          </template>
        </el-table-column>

        <el-table-column label="移除" width="80" align="center" fixed="right">
          <template #default="{ row }">
            <el-button 
              type="danger" 
              link
              class="delete-action-btn"
              @click="confirmDelete(row.id)"
            >
              <el-icon><Delete /></el-icon>
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- Upload Dialog -->
    <el-dialog v-model="showUploadDialog" title="注册新插件载荷" width="600px" class="premium-dialog" center>
      <div class="dialog-inner">
         <el-form label-position="top">
           <el-form-item label="插件标识名称 (Unique ID)" required>
              <el-input v-model="uploadForm.name" placeholder="例如: SharpKatz, Mimikatz, PortScanner..." />
           </el-form-item>
           <el-row :gutter="20">
              <el-col :span="12">
                 <el-form-item label="运行目标 OS" required>
                    <el-select v-model="uploadForm.required_os" style="width: 100%">
                       <el-option label="WINDOWS" value="windows" />
                       <el-option label="LINUX" value="linux" />
                       <el-option label="全平台 (MULTI)" value="multi" />
                    </el-select>
                 </el-form-item>
              </el-col>
              <el-col :span="12">
                 <el-form-item label="核心执行模式 (Execution Mode)" required>
                    <el-select v-model="uploadForm.type" style="width: 100%" @change="onTypeChange">
                       <el-option label="C# 反射负载 (CLR)" value="execute-assembly" />
                       <el-option label="内存匿名执行 (Memfd)" value="memfd-exec" />
                       <el-option label="Shellcode 注入 (RAW)" value="shellcode-inject" />
                       <el-option label="原生二进制运行" value="native-exec" />
                    </el-select>
                 </el-form-item>
              </el-col>
           </el-row>

           <el-form-item label="上传载荷文件 (.exe, .elf, .bin)" required>
              <el-upload 
                drag 
                action="#" 
                :auto-upload="false" 
                :limit="1" 
                :on-change="handleFileChange" 
                class="premium-uploader"
              >
                <div class="upload-v3">
                   <el-icon class="up-icon"><UploadFilled /></el-icon>
                   <div class="up-text">点击或拖拽载荷文件至此区域</div>
                   <div class="up-hint">系统将自动提取资源并注册至全局武器库</div>
                </div>
              </el-upload>
           </el-form-item>

           <el-form-item label="内部备注描述">
              <el-input v-model="uploadForm.description" type="textarea" :rows="2" placeholder="输入插件功能描述或使用提示..." />
           </el-form-item>
        </el-form>
      </div>
      <template #footer>
        <div class="dialog-footer">
           <el-button @click="showUploadDialog = false" class="plain-btn">放弃</el-button>
           <el-button type="primary" class="purple-btn" :loading="uploading" @click="submitUpload">建立插件索引并上传</el-button>
        </div>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { 
  Collection, Search, Upload, Delete, UploadFilled, Lock, Share, Box, 
  Platform, Cpu
} from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import api from '@/api'

const loading = ref(false)
const plugins = ref([])
const searchQuery = ref('')
const showUploadDialog = ref(false)
const uploading = ref(false)

const uploadForm = ref({
  name: '',
  description: '',
  required_os: 'windows',
  type: 'execute-assembly',
  category: 'general',
  file: null
})

const filteredPlugins = computed(() => {
  if (!searchQuery.value) return plugins.value
  const q = searchQuery.value.toLowerCase()
  return plugins.value.filter(p => 
    p.name.toLowerCase().includes(q) || 
    (p.description && p.description.toLowerCase().includes(q)) ||
    p.type.toLowerCase().includes(q)
  )
})

const onTypeChange = (type) => {
  if (type === 'memfd-exec') uploadForm.value.required_os = 'linux'
  if (type === 'execute-assembly' || type === 'shellcode-inject') uploadForm.value.required_os = 'windows'
}

const handleFileChange = (f) => { uploadForm.value.file = f.raw }

const fetchPlugins = async () => {
  loading.value = true
  try {
    const res = await api.get('/api/plugins')
    plugins.value = res.data || []
  } catch (e) { ElMessage.error('无法同步插件数据') }
  finally { loading.value = false }
}

const submitUpload = async () => {
  if (!uploadForm.value.file || !uploadForm.value.name) return ElMessage.warning('必要信息缺失')
  uploading.value = true
  const fd = new FormData()
  fd.append('file', uploadForm.value.file)
  fd.append('name', uploadForm.value.name)
  fd.append('description', uploadForm.value.description)
  fd.append('type', uploadForm.value.type)
  fd.append('required_os', uploadForm.value.required_os)
  fd.append('category', uploadForm.value.category)
  
  try {
    await api.post('/api/plugins/upload', fd, { headers: { 'Content-Type': 'multipart/form-data' } })
    ElMessage.success('插件注册成功')
    showUploadDialog.value = false
    fetchPlugins()
  } catch (e) { ElMessage.error('上传失败') }
  finally { uploading.value = false }
}

const confirmDelete = (id) => {
  ElMessageBox.confirm('确定将该插件从受控端武器库中移除？', '移除确认', { type: 'error' }).then(async () => {
    await api.delete(`/api/plugins/${id}`)
    ElMessage.success('已移除')
    fetchPlugins()
  })
}

const getOsTag = (os) => {
  if (os === 'windows') return 'primary'
  if (os === 'linux') return 'success'
  return 'info'
}

const getTypeTag = (type) => {
  const map = { 'execute-assembly': 'type-orange', 'memfd-exec': 'type-green', 'shellcode-inject': 'type-red' }
  return map[type] || 'type-grey'
}

const translateType = (type) => {
  const map = { 'execute-assembly': 'CLR 内存加载', 'memfd-exec': 'Linux memfd 执行', 'shellcode-inject': 'Shellcode 注入', 'native-exec': '宿主原生执行' }
  return map[type] || type
}

const translateCategory = (cat) => {
  const map = { 'credentials': '凭据窃取', 'lateral': '内网横向', 'privesc': '权限提升', 'general': '通用套件' }
  return map[cat] || '扩展插件'
}

const formatOS = (os) => {
  if (!os || os === 'multi') return 'ALL PLATFORMS'
  return os.toUpperCase()
}

onMounted(fetchPlugins)
</script>

<style scoped>
.plugin-management-container { padding: 0; animation: fadeIn 0.6s ease-out; }
@keyframes fadeIn { from { opacity: 0; transform: translateY(15px); } to { opacity: 1; transform: translateY(0); } }

.mb-24 { margin-bottom: 24px; }

/* Panes */
.glass-panel {
  background: rgba(255, 255, 255, 0.75);
  backdrop-filter: blur(20px); -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(124, 58, 237, 0.08); border-radius: 24px;
  box-shadow: 0 10px 30px rgba(124, 58, 237, 0.05);
}

.page-header { padding: 24px 32px; flex-shrink: 0; }
.header-content { display: flex; justify-content: space-between; align-items: center; gap: 30px; }
.main-title { font-size: 26px; font-weight: 900; color: #1e1b4b; margin: 0; white-space: nowrap; }
.purple-text { color: #7c3aed; }
.sub-title { font-size: 13px; color: #94a3b8; font-weight: 600; margin-top: 4px; }

.header-actions { display: flex; align-items: center; gap: 16px; flex: 1; justify-content: flex-end; }
.premium-search { width: 300px; }
:deep(.el-input__wrapper) { border-radius: 12px; border: 1px solid rgba(124, 58, 237, 0.1) !important; box-shadow: none !important; }

.premium-btn { border-radius: 12px; font-weight: 800; height: 42px; transition: all 0.2s; }
.upload-btn { background: #7c3aed !important; border: none !important; color: white !important; box-shadow: 0 4px 15px rgba(124, 58, 237, 0.2); }

/* Stats Row */
.stats-row { display: grid; grid-template-columns: repeat(3, 1fr); gap: 20px; }
.stat-module { padding: 20px; display: flex; align-items: center; gap: 16px; }
.stat-icon-box { width: 48px; height: 48px; border-radius: 14px; display: flex; align-items: center; justify-content: center; font-size: 22px; }
.stat-icon-box.purple { background: rgba(124, 58, 237, 0.1); color: #7c3aed; }
.stat-icon-box.blue { background: rgba(14, 165, 233, 0.1); color: #0ea5e9; }
.stat-icon-box.orange { background: rgba(245, 158, 11, 0.1); color: #f59e0b; }
.stat-label { font-size: 11px; font-weight: 800; color: #94a3b8; text-transform: uppercase; letter-spacing: 0.5px; }
.stat-value { font-family: 'JetBrains Mono'; font-size: 26px; font-weight: 800; color: #1e1b4b; }

/* Table */
.table-module { padding: 12px; }
.premium-table { background: transparent !important; }

.category-orb { width: 34px; height: 34px; border-radius: 10px; display: flex; align-items: center; justify-content: center; font-size: 16px; }
.category-orb.credentials { background: #fee2e2; color: #ef4444; }
.category-orb.lateral { background: #ecfeff; color: #0891b2; }
.category-orb.general { background: #f1f5f9; color: #64748b; }

.name-cell { display: flex; flex-direction: column; gap: 2px; }
.p-name { font-weight: 800; color: #1e1b4b; font-size: 14px; }
.p-category { font-size: 10px; color: #94a3b8; font-weight: 800; text-transform: uppercase; }

.desc-text { color: #64748b; font-size: 13px; font-weight: 600; }
.type-capsule { display: inline-block; padding: 4px 10px; border-radius: 8px; font-size: 11px; font-weight: 900; }
.type-orange { background: rgba(245, 158, 11, 0.1); color: #d97706; }
.type-green { background: rgba(16, 185, 129, 0.1); color: #059669; }
.type-red { background: rgba(239, 68, 68, 0.1); color: #ef4444; }
.type-grey { background: #f8fafc; color: #94a3b8; }

.delete-action-btn { font-size: 18px; color: #cbd5e1 !important; transition: all 0.2s; }
.delete-action-btn:hover { color: #ef4444 !important; }

.dialog-inner { padding: 15px 10px; }

/* 深度优化输入框 & 表单间距 */
:deep(.el-form-item) {
  margin-bottom: 24px; /* 增加表单项之间的垂直间距 */
}

:deep(.el-form-item__label) {
  font-weight: 800;
  color: #1e1b4b;
  padding-bottom: 8px;
  font-size: 13px;
}

:deep(.el-input__wrapper), :deep(.el-textarea__wrapper) {
  background-color: #f8fafc !important;
  border: 1px solid #e2e8f0 !important;
  box-shadow: none !important;
  padding: 8px 12px;
  border-radius: 12px;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

:deep(.el-select .el-input__wrapper) {
  padding: 4px 12px;
}

:deep(.el-input__wrapper.is-focus), :deep(.el-textarea__wrapper.is-focus) {
  border-color: #7c3aed !important;
  background-color: #ffffff !important;
  box-shadow: 0 0 0 4px rgba(124, 58, 237, 0.08) !important;
}

:deep(.el-input__inner), :deep(.el-textarea__inner) {
  color: #1e1b4b;
  font-weight: 600;
}

.platform-tabs { width: 100%; display: flex; gap: 8px; }
:deep(.el-radio-button) { flex: 1; border-radius: 10px; overflow: hidden; }
:deep(.el-radio-button__inner) { 
  width: 100%; 
  font-weight: 800; 
  padding: 12px 0; 
  border: 1px solid #e2e8f0 !important;
  border-radius: 10px !important;
  background: #f8fafc;
  color: #94a3b8;
  transition: all 0.2s;
}

:deep(.el-radio-button__original-radio:checked + .el-radio-button__inner) {
  background: #7c3aed !important;
  color: white !important;
  border-color: #7c3aed !important;
  box-shadow: 0 4px 12px rgba(124, 58, 237, 0.2) !important;
}

.premium-uploader { width: 100%; margin-top: 5px; }
:deep(.el-upload-dragger) { 
  border: 2px dashed rgba(124, 58, 237, 0.2); 
  border-radius: 20px; 
  background: rgba(124, 58, 237, 0.02); 
  padding: 40px 20px;
  transition: all 0.2s; 
}
:deep(.el-upload-dragger:hover) { border-color: #7c3aed; background: rgba(124, 58, 237, 0.05); }

.upload-v3 { display: flex; flex-direction: column; align-items: center; }
.up-icon { font-size: 48px; color: #7c3aed; margin-bottom: 16px; opacity: 0.8; }
.up-text { font-size: 15px; font-weight: 900; color: #1e1b4b; margin-bottom: 8px; }
.up-hint { font-size: 12px; color: #94a3b8; font-weight: 600; }

.dialog-footer { display: flex; justify-content: flex-end; gap: 15px; border-top: 1px solid #f1f5f9; padding-top: 25px; margin-top: 10px; }
.plain-btn { border-radius: 12px; font-weight: 800; padding: 0 25px; height: 44px; }
.purple-btn { background: #7c3aed !important; border: none !important; color: white !important; font-weight: 900; border-radius: 12px; padding: 0 30px; height: 44px; box-shadow: 0 8px 20px rgba(124, 58, 237, 0.2); }
</style>
