<template>
  <div class="settings-page-container">
    <!-- Page Header -->
    <div class="page-header glass-panel mb-24">
      <div class="header-content">
        <div class="title-section">
          <h2 class="main-title">系统核心 <span class="purple-text">全局设置</span></h2>
          <p class="sub-title">平台安全性与自动化集成</p>
        </div>
      </div>
    </div>

    <!-- Main Settings Card -->
    <div class="main-body glass-panel">
      <el-tabs v-model="activeTab" class="premium-tabs">
        <!-- 1. Operator Management -->
        <el-tab-pane name="users">
          <template #label>
            <div class="tab-label">
               <el-icon><User /></el-icon>
               <span>人员与访问控制</span>
            </div>
          </template>
          
          <div class="tab-inner">
            <div class="section-title-line mb-20">
              <h3 class="section-h3">后台操作员列表</h3>
              <el-button class="premium-btn purple-btn" :icon="Plus" @click="openUserDialog()">新增资产操作员</el-button>
            </div>
            
            <el-table :data="users" v-loading="loading" class="premium-table">
              <el-table-column prop="username" label="账户名">
                 <template #default="scope">
                    <div class="user-id-cell">
                       <el-avatar :size="24" class="mini-avatar">{{ scope.row.username.charAt(0).toUpperCase() }}</el-avatar>
                       <span class="u-name">{{ scope.row.username }}</span>
                    </div>
                 </template>
              </el-table-column>
              <el-table-column label="角色权限" width="140" align="center">
                <template #default="scope">
                  <div class="role-chip" :class="scope.row.role">
                    {{ scope.row.role === 'admin' ? '系统管理员' : '战术操作员' }}
                  </div>
                </template>
              </el-table-column>
              <el-table-column label="账号状态" width="120" align="center">
                <template #default="scope">
                  <el-switch 
                    v-model="scope.row.is_active" 
                    @change="toggleUserStatus(scope.row)"
                    active-color="#10b981"
                  />
                </template>
              </el-table-column>
              <el-table-column label="操作" width="180" align="center">
                <template #default="scope">
                  <el-button link class="action-btn purple" @click="openUserDialog(scope.row)">鉴权变更</el-button>
                  <el-button 
                    link 
                    class="action-btn red"
                    @click="deleteUser(scope.row)" 
                    :disabled="scope.row.username === 'admin'"
                  >注销</el-button>
                </template>
              </el-table-column>
            </el-table>

            <el-divider class="section-divider" />
            
            <h3 class="section-h3 mb-20">登录审计流</h3>
            <el-table :data="loginLogs" size="small" class="premium-table audit-table">
              <el-table-column prop="created_at" label="时间戳" width="180">
                <template #default="scope">{{ formatDate(scope.row.created_at) }}</template>
              </el-table-column>
              <el-table-column prop="username" label="操作账户" width="120" />
              <el-table-column prop="ip" label="源 IP" width="140" />
              <el-table-column label="结果状态" width="100">
                <template #default="scope">
                  <div class="audit-status" :class="scope.row.status">
                    {{ scope.row.status === 'success' ? '通过' : '拒绝' }}
                  </div>
                </template>
              </el-table-column>
              <el-table-column prop="user_agent" label="终端环境代理" show-overflow-tooltip />
            </el-table>
          </div>
        </el-tab-pane>

        <!-- 2. Webhooks -->
        <el-tab-pane name="notifications">
          <template #label>
            <div class="tab-label">
               <el-icon><Bell /></el-icon>
               <span>自动化通知集</span>
            </div>
          </template>
          
          <div class="tab-inner">
            <div class="section-title-line mb-20">
              <h3 class="section-h3">外部推送隧道</h3>
              <el-button class="premium-btn purple-btn" :icon="Plus" @click="openWebhookDialog()">接入新 Webhook</el-button>
            </div>
            
            <el-row :gutter="20">
              <el-col :span="12" v-for="hook in webhooks" :key="hook.id">
                <div class="webhook-bento-card">
                   <div class="bento-header">
                      <div class="bento-logo-box">
                         <img :src="getWebhookIcon(hook.type)" class="bento-icon" />
                         <span>{{ hook.name }}</span>
                      </div>
                      <el-switch v-model="hook.is_enabled" @change="saveWebhook(hook)" active-color="#10b981" />
                   </div>
                   <div class="bento-url">{{ hook.url }}</div>
                   <div class="bento-footer">
                      <div class="event-chips">
                        <span v-for="ev in hook.events.split(',')" :key="ev" class="mini-chip">
                           {{ ev === 'agent_online' ? '⚡ 上线推送' : '🔌 掉线告警' }}
                        </span>
                      </div>
                      <div class="bento-actions">
                         <el-button link class="action-btn purple" @click="openWebhookDialog(hook)">配置</el-button>
                         <el-button link class="action-btn red" @click="deleteWebhook(hook.id)">注销</el-button>
                      </div>
                   </div>
                </div>
              </el-col>
            </el-row>
          </div>
        </el-tab-pane>

        <!-- 3. Global Configuration -->
        <el-tab-pane name="policies">
          <template #label>
            <div class="tab-label">
               <el-icon><Setting /></el-icon>
               <span>核心运行策略</span>
            </div>
          </template>
          
          <div class="tab-inner policy-form">
            <el-form label-position="top">
               <div class="form-grid-v2">
                 <div class="form-group glass-panel-sub">
                    <label class="group-label">回连基础参数</label>
                    <el-row :gutter="20">
                       <el-col :span="12">
                          <el-form-item label="默认心跳频率 (秒)">
                             <el-input-number v-model="globalConfig.default_sleep" :min="1" style="width: 100%" />
                          </el-form-item>
                       </el-col>
                       <el-col :span="12">
                          <el-form-item label="心跳抖动浮动 (%)">
                             <el-input-number v-model="globalConfig.default_jitter" :min="0" :max="100" style="width: 100%" />
                          </el-form-item>
                       </el-col>
                    </el-row>
                 </div>

                 <div class="form-group glass-panel-sub">
                    <label class="group-label">安全特征伪装</label>
                    <el-form-item label="全局反连地址 Host">
                       <el-input v-model="globalConfig.system_c2_host" placeholder="c2.domain.com" />
                    </el-form-item>
                    <el-form-item label="探测屏蔽重定向 (404 Cloak URL)">
                       <el-input v-model="globalConfig.opsec_cloak_url" placeholder="https://www.bing.com" />
                    </el-form-item>
                 </div>

                 <div class="form-group glass-panel-sub">
                    <label class="group-label">鉴权与自动化</label>
                    <el-form-item label="Master API Token">
                       <el-input v-model="globalConfig.system_api_token" show-password>
                          <template #append>
                             <el-button @click="copyToken">复制</el-button>
                          </template>
                       </el-input>
                    </el-form-item>
                    <div class="switch-row">
                       <div class="row-label">
                          <span>启用 MCP 自动化网关</span>
                          <small>允许外部脚本通过 Token 访问接口</small>
                       </div>
                       <el-switch v-model="globalConfig.system_mcp_enabled" active-value="true" inactive-value="false" active-color="#7c3aed" />
                    </div>
                 </div>
               </div>

               <div class="form-footer-action">
                  <el-button type="primary" class="huge-save-btn" @click="saveGlobalSettings">同步核心配置至集群</el-button>
               </div>
            </el-form>
          </div>
        </el-tab-pane>

        <!-- 4. Maintenance -->
        <el-tab-pane name="maintenance">
          <template #label>
            <div class="tab-label">
               <el-icon><DataLine /></el-icon>
               <span>数据维护与熔断</span>
            </div>
          </template>
          
          <div class="tab-inner">
             <div class="maintenance-grid">
                <div class="m-card glass-panel-sub">
                   <div class="m-icon blue"><el-icon><Download /></el-icon></div>
                   <h4 class="m-title">全量数据冷备份</h4>
                   <p class="m-desc">导出当前数据库所有资产标识、通信日志及任务审计历史为 JSON 格式。</p>
                   <el-button plain class="m-btn" @click="exportData">执行全量导出</el-button>
                </div>
                <div class="m-card glass-panel-sub">
                   <div class="m-icon red"><el-icon><Delete /></el-icon></div>
                   <h4 class="m-title">环境一键熔断</h4>
                   <p class="m-desc">立即清除所有 Agent 回连记录、历史指令流。此操作不可逆。</p>
                   <el-button type="danger" class="m-btn" @click="resetDatabase">紧急熔断环境</el-button>
                </div>
             </div>
          </div>
        </el-tab-pane>
      </el-tabs>
    </div>

    <!-- User Modal -->
    <el-dialog v-model="userDialog.visible" :title="userDialog.isEdit ? '人员鉴权变更' : '人员准入授权'" width="420px" class="premium-dialog" center>
      <div class="dialog-inner">
         <el-form :model="userDialog.form" label-position="top">
            <el-form-item label="操作员 ID">
               <el-input v-model="userDialog.form.username" :disabled="userDialog.isEdit" prefix-icon="User" />
            </el-form-item>
            <el-form-item label="访问密文 (密码)">
               <el-input v-model="userDialog.form.password" type="password" show-password placeholder="保持不变请留空" prefix-icon="Lock" />
            </el-form-item>
            <el-form-item label="授权角色">
               <el-select v-model="userDialog.form.role" style="width: 100%">
                  <el-option label="系统管理员" value="admin" />
                  <el-option label="战术操作员" value="operator" />
               </el-select>
            </el-form-item>
         </el-form>
      </div>
      <template #footer>
         <div class="dialog-footer">
            <el-button @click="userDialog.visible = false" class="plain-btn">取消</el-button>
            <el-button type="primary" class="purple-btn" @click="saveUser">确认同步</el-button>
         </div>
      </template>
    </el-dialog>

    <!-- Webhook Modal -->
    <el-dialog v-model="webhookDialog.visible" title="Webhook 通道集成" width="500px" class="premium-dialog" center>
      <div class="dialog-inner">
         <el-form :model="webhookDialog.form" label-position="top">
            <el-form-item label="通道描述名称">
               <el-input v-model="webhookDialog.form.name" placeholder="例如: 蓝队预警频道" />
            </el-form-item>
            <el-form-item label="集成协议类型">
               <el-radio-group v-model="webhookDialog.form.type" class="platform-tabs">
                  <el-radio-button label="dingtalk">钉钉</el-radio-button>
                  <el-radio-button label="feishu">飞书</el-radio-button>
                  <el-radio-button label="telegram">TG</el-radio-button>
               </el-radio-group>
            </el-form-item>
            <el-form-item label="转发接口 URL (Callback URL)">
               <el-input v-model="webhookDialog.form.url" type="textarea" :rows="2" placeholder="https://oapi.dingtalk.com/..." />
            </el-form-item>
         </el-form>
      </div>
      <template #footer>
         <div class="dialog-footer">
            <el-button @click="webhookDialog.visible = false" class="plain-btn">取消</el-button>
            <el-button type="primary" class="purple-btn" @click="submitWebhook">激活通道</el-button>
         </div>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, reactive, onMounted } from 'vue'
import { 
  User, Bell, Setting, Plus, Download, Delete, DataLine, Lock, Key 
} from '@element-plus/icons-vue'
import api from '../api/index'
import { ElMessage, ElMessageBox } from 'element-plus'

const activeTab = ref('users')
const loading = ref(false)

const users = ref([])
const loginLogs = ref([])
const webhooks = ref([])
const globalConfig = reactive({
  default_sleep: 60, default_jitter: 10, system_c2_host: '',
  system_api_token: '', system_mcp_enabled: 'true', opsec_cloak_url: '',
  web_auth_user: 'admin', web_auth_password: 'cupcake', allowed_ips: ''
})

const userDialog = reactive({ visible: false, isEdit: false, form: { id: null, username: '', password: '', role: 'operator' } })
const webhookDialog = reactive({ visible: false, isEdit: false, form: { id: null, name: '', type: 'dingtalk', url: '', events: '' }, selectedEvents: ['agent_online'] })

const fetchAll = async () => {
    loading.value = true
    try {
        const [u, logs, hooks, conf] = await Promise.all([
            api.get('/api/settings/users'), api.get('/api/settings/logs/login'),
            api.get('/api/settings/webhooks'), api.get('/api/settings/config')
        ])
        users.value = u.data || []
        loginLogs.value = logs.data || []
        webhooks.value = hooks.data || []
        conf.data.forEach(item => {
            if (globalConfig.hasOwnProperty(item.key)) {
                if (['default_sleep', 'default_jitter'].includes(item.key)) globalConfig[item.key] = parseInt(item.value)
                else globalConfig[item.key] = item.value
            }
        })
    } catch (e) { ElMessage.error('同步异常') }
    finally { loading.value = false }
}

const openUserDialog = (row = null) => {
    userDialog.isEdit = !!row
    userDialog.form = row ? { ...row, password: '' } : { id: null, username: '', password: '', role: 'operator' }
    userDialog.visible = true
}

const saveUser = async () => {
    try {
        if (userDialog.isEdit) await api.put(`/api/settings/users/${userDialog.form.id}`, userDialog.form)
        else await api.post('/api/settings/users', userDialog.form)
        userDialog.visible = false
        fetchAll()
    } catch (e) { ElMessage.error('无法同步账户变更') }
}

const toggleUserStatus = async (user) => {
    try { await api.put(`/api/settings/users/${user.id}`, { is_active: user.is_active }) }
    catch (e) { user.is_active = !user.is_active; ElMessage.error('变更受阻') }
}

const deleteUser = (user) => {
    ElMessageBox.confirm(`注销操作员 ${user.username}？`, '核心警告', { type: 'warning' }).then(async () => {
        await api.delete(`/api/settings/users/${user.id}`)
        fetchAll()
    })
}

const openWebhookDialog = (row = null) => {
    webhookDialog.form = row ? { ...row } : { id: null, name: '', type: 'dingtalk', url: '', events: 'agent_online' }
    webhookDialog.visible = true
}

const submitWebhook = async () => {
    webhookDialog.form.events = 'agent_online,agent_offline'
    await saveWebhook(webhookDialog.form)
    webhookDialog.visible = false
}

const saveWebhook = async (hook) => {
    try { await api.post('/api/settings/webhooks', hook); fetchAll() }
    catch (e) { ElMessage.error('Webhook 同步失败') }
}

const deleteWebhook = (id) => {
    api.delete(`/api/settings/webhooks/${id}`).then(() => fetchAll())
}

const getWebhookIcon = (type) => {
    const icons = {
        dingtalk: 'https://img.icons8.com/color/48/000000/dingtalk.png',
        feishu: 'https://img.icons8.com/color/48/000000/lark.png',
        slack: 'https://img.icons8.com/color/48/000000/slack-new.png',
        telegram: 'https://img.icons8.com/color/48/000000/telegram-app.png'
    }
    return icons[type] || ''
}

const saveGlobalSettings = async () => {
    const payload = Object.entries(globalConfig).map(([key, value]) => {
        let group = 'access'
        if (key.startsWith('opsec')) group = 'opsec'
        else if (key.startsWith('default')) group = 'general'
        else if (key.includes('token')) group = 'security'
        return { key, value: String(value), group }
    })
    try { await api.post('/api/settings/config', payload); ElMessage.success('配置同步成功') }
    catch (e) { ElMessage.error('保存同步冲突') }
}

const copyToken = () => { navigator.clipboard.writeText(globalConfig.system_api_token); ElMessage.success('Token 已复制') }

const exportData = () => { window.open('/api/maintenance/export', '_blank') }

const resetDatabase = () => {
    ElMessageBox.confirm('环境熔断将清空所有战利品记录，确定继续？', '熔断确认', { type: 'error' }).then(async () => {
        await api.post('/api/maintenance/reset')
        fetchAll()
    })
}

const formatDate = (ts) => ts ? new Date(ts).toLocaleString() : '---'

onMounted(fetchAll)
</script>

<style scoped>
.settings-page-container { padding: 0; animation: fadeIn 0.6s ease-out; }
@keyframes fadeIn { from { opacity: 0; transform: translateY(15px); } to { opacity: 1; transform: translateY(0); } }

.mb-24 { margin-bottom: 24px; }
.mb-20 { margin-bottom: 20px; }
.mt-15 { margin-top: 15px; }

/* Panes */
.glass-panel {
  background: rgba(255, 255, 255, 0.75);
  backdrop-filter: blur(20px); -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(124, 58, 237, 0.08); border-radius: 24px;
  box-shadow: 0 10px 30px rgba(124, 58, 237, 0.05); padding: 0; overflow: hidden;
}

.glass-panel-sub {
  background: rgba(124, 58, 237, 0.02);
  border: 1px solid rgba(124, 58, 237, 0.05); border-radius: 20px;
  padding: 24px;
}

.page-header { padding: 24px 32px; flex-shrink: 0; }
.header-content { display: flex; justify-content: space-between; align-items: center; }
.main-title { font-size: 26px; font-weight: 900; color: #1e1b4b; margin: 0; }
.purple-text { color: #7c3aed; }
.sub-title { font-size: 13px; color: #94a3b8; font-weight: 600; margin-top: 4px; }

/* Tabs */
.premium-tabs :deep(.el-tabs__header) { background: #f8fafc; padding: 0 20px; margin: 0; border-bottom: 1px solid rgba(124, 58, 237, 0.05); }
.premium-tabs :deep(.el-tabs__item) { height: 60px; line-height: 60px; font-weight: 800; font-size: 13px; color: #94a3b8; }
.premium-tabs :deep(.el-tabs__item.is-active) { color: #7c3aed; }
.premium-tabs :deep(.el-tabs__active-bar) { height: 3px; border-radius: 3px; background-color: #7c3aed; }
.tab-label { display: flex; align-items: center; gap: 8px; }

.tab-inner { padding: 32px; }

/* Tables */
.section-title-line { display: flex; justify-content: space-between; align-items: center; }
.section-h3 { font-size: 18px; font-weight: 900; color: #1e1b4b; margin: 0; }
.premium-table { background: transparent !important; }

.user-id-cell { display: flex; align-items: center; gap: 10px; }
.mini-avatar { background: #7c3aed; color: white; font-weight: 800; }
.u-name { font-weight: 800; color: #1e1b4b; font-size: 14px; }

.role-chip { display: inline-block; padding: 4px 12px; border-radius: 8px; font-size: 11px; font-weight: 800; }
.role-chip.admin { background: rgba(124, 58, 237, 0.1); color: #7c3aed; }
.role-chip.operator { background: rgba(16, 185, 129, 0.1); color: #059669; }

.action-btn { font-size: 13px; font-weight: 800; }
.action-btn.purple { color: #7c3aed !important; }
.action-btn.red { color: #ef4444 !important; }

.audit-table { border-radius: 12px; overflow: hidden; border: 1px solid #f1f5f9; }
.audit-status { font-weight: 900; font-size: 11px; }
.audit-status.success { color: #10b981; }
.audit-status.failed { color: #ef4444; }

/* Webhooks */
.webhook-bento-card { background: #f8fafc; border-radius: 20px; padding: 20px; border: 1px solid transparent; transition: all 0.2s; margin-bottom: 20px; }
.webhook-bento-card:hover { transform: translateY(-3px); border-color: rgba(124, 58, 237, 0.1); background: white; box-shadow: 0 10px 25px rgba(124, 58, 237, 0.05); }
.bento-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
.bento-logo-box { display: flex; align-items: center; gap: 10px; font-weight: 900; color: #1e1b4b; font-size: 14px; }
.bento-icon { width: 24px; height: 24px; object-fit: contain; }
.bento-url { font-family: 'JetBrains Mono'; font-size: 11px; color: #94a3b8; background: #f1f5f9; padding: 8px 12px; border-radius: 10px; margin-bottom: 15px; word-break: break-all; }
.bento-footer { display: flex; justify-content: space-between; align-items: center; }
.mini-chip { font-size: 10px; font-weight: 800; color: #64748b; background: white; padding: 2px 8px; border-radius: 6px; border: 1px solid #e2e8f0; margin-right: 5px; }

/* Policies */
.form-grid-v2 { display: grid; grid-template-columns: repeat(2, 1fr); gap: 24px; }
.group-label { display: block; font-size: 11px; font-weight: 900; color: #7c3aed; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 24px; }
.switch-row { display: flex; justify-content: space-between; align-items: center; padding: 16px 0; border-top: 1px solid rgba(124, 58, 237, 0.05); }
.row-label { display: flex; flex-direction: column; }
.row-label span { font-size: 14px; font-weight: 800; color: #1e1b4b; }
.row-label small { font-size: 11px; color: #94a3b8; font-weight: 600; }
.huge-save-btn { width: 100%; height: 50px; border-radius: 16px; font-weight: 900; font-size: 15px; margin-top: 32px; background: #7c3aed !important; border: none !important; box-shadow: 0 10px 25px rgba(124, 58, 237, 0.3); }

/* Maintenance */
.maintenance-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 24px; }
.m-card { display: flex; flex-direction: column; align-items: center; text-align: center; padding: 40px 24px; }
.m-icon { width: 60px; height: 60px; border-radius: 20px; display: flex; align-items: center; justify-content: center; font-size: 28px; margin-bottom: 20px; }
.m-icon.blue { background: rgba(14, 165, 233, 0.1); color: #0ea5e9; }
.m-icon.red { background: rgba(239, 68, 68, 0.1); color: #ef4444; }
.m-title { font-size: 18px; font-weight: 900; color: #1e1b4b; margin: 0 0 10px 0; }
.m-desc { font-size: 13px; color: #64748b; line-height: 1.6; margin-bottom: 30px; max-width: 250px; }
.m-btn { width: 100%; border-radius: 12px; font-weight: 800; height: 42px; }

/* Modal */
.platform-tabs { width: 100%; display: flex; }
:deep(.el-radio-button) { flex: 1; }
:deep(.el-radio-button__inner) { width: 100%; font-weight: 800; padding: 12px 0; border-radius: 10px !important; }
.purple-btn { background: #7c3aed !important; border: none !important; color: white !important; font-weight: 800; border-radius: 10px; padding: 0 20px; height: 42px; transition: all 0.2s; }
.plain-btn { border-radius: 10px; font-weight: 700; }
</style>
