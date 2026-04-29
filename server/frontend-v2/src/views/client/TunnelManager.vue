<template>
  <div class="tunnel-manager">
    <el-card shadow="never">
      <template #header>
        <div class="card-header">
          <span>
             <el-icon style="vertical-align: middle; margin-right: 5px;"><Connection /></el-icon>
             内网隧道
          </span>
          <div>
              <el-button :icon="Refresh" circle @click="fetchTunnels" style="margin-right: 10px"/>
              <el-button type="primary" :icon="Plus" @click="handleAdd">新建隧道</el-button>
          </div>
        </div>
      </template>

      <el-table v-loading="listLoading" :data="clientTunnels" style="width: 100%" empty-text="暂无活动隧道">
        <el-table-column label="协议" width="100">
          <template #default="scope">
            <el-tag :type="scope.row.type === 'http' ? 'warning' : 'success'" effect="dark">
              {{ scope.row.type ? scope.row.type.toUpperCase() : 'SOCKS5' }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column label="监听地址" width="180">
          <template #default="scope">
            <el-tag type="info" class="addr-code">0.0.0.0:{{ scope.row.port }}</el-tag>
          </template>
        </el-table-column>

        <el-table-column label="转发目标" min-width="200">
          <template #default>
            <span style="color: #606266;">动态转发</span>
          </template>
        </el-table-column>

        <el-table-column label="状态" width="100">
          <template #default="scope">
            <el-tag type="success" v-if="scope.row.status === 'running'">运行中</el-tag>
            <el-tag type="danger" v-else>已停止</el-tag>
          </template>
        </el-table-column>

        <el-table-column label="操作" width="100" align="center">
          <template #default="scope">
             <el-button type="danger" size="small" @click="handleStopTunnel(scope.row.port)">停止</el-button>
          </template>
        </el-table-column>
      </el-table>

      <el-empty v-if="clientTunnels.length === 0" description="该客户端暂无活动隧道">
        <el-button type="primary" @click="handleAdd">开启第一个隧道</el-button>
      </el-empty>
    </el-card>

    <!-- Create/Edit Tunnel Dialog -->
    <el-dialog 
      :title="isEdit ? '编辑网络隧道' : '新建网络隧道'" 
      v-model="dialogVisible" 
      width="480px" 
      destroy-on-close
      center
    >
      <el-alert
        v-if="!isEdit"
        title="功能说明"
        type="info"
        :closable="false"
        show-icon
        description="在服务端 启动一个监听端口，将流量透明转发至该终端所在的内网环境。适用于内网渗透、扫描等场景。"
        style="margin-bottom: 20px;"
      />

      <el-form label-position="top">
        <el-form-item label="服务端监听端口">
          <el-input-number 
            v-model="form.port" 
            :min="1" 
            :max="65535" 
            controls-position="right"
            style="width: 100%;" 
            placeholder="例如: 1080"
          />
        </el-form-item>

        <el-form-item label="隧道协议">
          <el-radio-group v-model="form.type" style="width: 100%; display: flex;">
            <el-radio-button label="socks5" style="flex: 1; text-align: center;">
              SOCKS5
            </el-radio-button>
            <el-radio-button label="http" style="flex: 1; text-align: center;">
              HTTP / HTTPS
            </el-radio-button>
          </el-radio-group>
        </el-form-item>

        <el-divider content-position="center">
          <el-icon style="vertical-align: middle; margin-right: 5px;"><Lock /></el-icon>
          安全设置
        </el-divider>

        <el-form-item label="身份验证">
          <el-switch 
            v-model="form.enableAuth" 
            active-text="启用账号密码" 
            inactive-text="无认证"
            style="--el-switch-on-color: #13ce66;"
          />
        </el-form-item>

        <transition name="el-zoom-in-top">
          <div v-if="form.enableAuth" class="auth-box">
            <el-row :gutter="15">
              <el-col :span="12">
                <el-form-item label="用户名">
                  <el-input 
                    v-model="form.username" 
                    placeholder="例如: admin" 
                    :prefix-icon="User"
                  />
                </el-form-item>
              </el-col>
              <el-col :span="12">
                <el-form-item label="密码">
                  <el-input 
                    v-model="form.password" 
                    type="password" 
                    show-password 
                    placeholder="设置强密码" 
                    :prefix-icon="Key"
                  />
                </el-form-item>
              </el-col>
            </el-row>
            <el-alert title="提示：认证配置将同时应用于 SOCKS5/HTTP 代理。" type="info" :closable="false" show-icon />
          </div>
        </transition>
      </el-form>

      <template #footer>
        <span class="dialog-footer">
          <el-button @click="dialogVisible = false">取 消</el-button>
          <el-button type="primary" @click="submitTunnel" :loading="submitting">
            {{ isEdit ? '保 存' : '立即开启' }}
          </el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, reactive, defineProps, onMounted, computed } from 'vue'
import { Plus, Refresh, Connection, ArrowDown, VideoPlay, VideoPause, Edit, Delete, Lock, User, Key } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { getActiveTunnels, startTunnel, stopTunnel, deleteTunnel } from '@/api/socks'

const props = defineProps({
  clientId: {
    type: String,
    required: true
  },
  clientInfo: {
    type: Object,
    default: null
  }
})

const tunnels = ref([]) // Global tunnels
const dialogVisible = ref(false)
const submitting = ref(false)
const listLoading = ref(false)
const isEdit = ref(false)
const oldPort = ref('')

// Filter for THIS client
const clientTunnels = computed(() => {
    return tunnels.value.filter(t => t.agent_id === props.clientId)
})

const form = reactive({
    port: 1080,
    type: 'socks5',
    enableAuth: false,
    username: '',
    password: ''
})

const fetchTunnels = async () => {
    listLoading.value = true
    try {
        const res = await getActiveTunnels()
        if (res.data && res.data.tunnels) {
            tunnels.value = res.data.tunnels
        } else {
            tunnels.value = []
        }
    } catch (e) {
        ElMessage.warning('无法同步隧道列表')
    } finally {
        listLoading.value = false
    }
}

const handleCommand = (command, row) => {
  switch (command) {
    case 'start':
      handleRestart(row)
      break
    case 'stop':
      handleStopTunnel(row.port)
      break
    case 'edit':
      handleEdit(row)
      break
    case 'delete':
      handleDelete(row.port)
      break
  }
}

const handleAdd = () => {
    isEdit.value = false
    form.port = 1080
    form.type = 'socks5'
    form.enableAuth = false
    form.username = ''
    form.password = ''
    dialogVisible.value = true
}

const handleRestart = async (row) => {
    try {
        await startTunnel({
            uuid: props.clientId,
            port: String(row.port),
            type: row.type
        })
        ElMessage.success('隧道已启动')
        fetchTunnels()
    } catch (e) {
        ElMessage.error(e.response?.data?.message || '启动失败')
    }
}

const submitTunnel = async () => {
  submitting.value = true
    if (form.enableAuth && (!form.username || !form.password)) {
        ElMessage.warning('启用认证时，用户名和密码不能为空')
        submitting.value = false
        return
    }

    try {
        await startTunnel({
            uuid: props.clientId,
            port: String(form.port),
            type: form.type,
            username: form.enableAuth ? form.username : '',
            password: form.enableAuth ? form.password : ''
        })

    if (isEdit.value && String(form.port) !== oldPort.value) {
        await deleteTunnel({ port: oldPort.value })
    }

    ElMessage.success(isEdit.value ? '配置已更新' : `${form.type.toUpperCase()} 代理已启动`)
    dialogVisible.value = false
    fetchTunnels()
  } catch (e) {
    const msg = e.response?.data?.message || '操作失败'
    ElMessage.error(msg)
  } finally {
    submitting.value = false
  }
}

const handleStopTunnel = async (port) => {
    try {
        await stopTunnel({ port: port })
        ElMessage.success('隧道已停止')
        fetchTunnels()
    } catch (e) {
        ElMessage.error('停止失败')
    }
}

const handleDelete = async (port) => {
    try {
        await ElMessageBox.confirm('确定要彻底删除该隧道配置吗？', '警告', {
            confirmButtonText: '删除',
            cancelButtonText: '取消',
            type: 'warning'
        })
        await deleteTunnel({ port: port })
        ElMessage.success('删除成功')
        fetchTunnels()
    } catch (e) {}
}

const handleEdit = (row) => {
    isEdit.value = true
    oldPort.value = row.port
    form.port = parseInt(row.port)
    form.type = row.type || 'socks5'
    form.username = row.username || ''
    form.password = row.password || ''
    form.enableAuth = !!(form.username && form.password)
    dialogVisible.value = true
}

onMounted(() => {
    fetchTunnels()
})
</script>

<style scoped>
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.addr-code {
  background: rgba(64, 158, 255, 0.1);
  padding: 2px 6px;
  border-radius: 4px;
  color: var(--primary-color);
  font-family: 'JetBrains Mono', monospace;
  border: 1px solid rgba(64, 158, 255, 0.2);
}

/* Force radio buttons to fill width evenly */
:deep(.el-radio-button) {
  flex: 1;
}
:deep(.el-radio-button__inner) {
  width: 100%;
  padding: 12px 0; 
}
:deep(.el-radio-group) {
  width: 100%;
}
.auth-box {
  background-color: rgba(0, 0, 0, 0.05);
  padding: 15px;
  border-radius: 4px;
  border: 1px dashed rgba(64, 158, 255, 0.3);
  margin-bottom: 20px;
}
</style>
