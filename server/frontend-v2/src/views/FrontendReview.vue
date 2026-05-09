<template>
  <div class="view-shell review-shell">
    <section class="review-meta">
      <div class="chip">临时审查入口</div>
      <div class="chip">按页面分组</div>
    </section>

    <section class="review-grid">
      <article class="surface-card review-card">
        <div class="card-head">
          <div>
            <span class="card-kicker">Updated</span>
            <h3>已经统一的页面</h3>
          </div>
        </div>

        <div class="link-stack">
          <button v-for="item in updatedPages" :key="item.route" type="button" class="review-link" @click="go(item.route)">
            <div>
              <strong>{{ item.title }}</strong>
              <p>{{ item.note }}</p>
            </div>
            <span>查看</span>
          </button>
        </div>
      </article>

      <article class="surface-card review-card">
        <div class="card-head">
          <div>
            <span class="card-kicker">Next Up</span>
            <h3>建议继续处理</h3>
          </div>
        </div>

        <div class="todo-stack">
          <div v-for="item in pendingPages" :key="item.title" class="todo-item">
            <strong>{{ item.title }}</strong>
            <p>{{ item.note }}</p>
          </div>
        </div>
      </article>
    </section>

    <section class="surface-card quick-panel">
      <div class="card-head">
        <div>
          <span class="card-kicker">Quick Actions</span>
          <h3>快速跳转</h3>
        </div>
      </div>

      <div class="quick-actions">
        <el-button @click="go('/dashboard')">仪表盘</el-button>
        <el-button @click="go('/clients')">受控端</el-button>
        <el-button @click="go('/listeners')">监听器</el-button>
        <el-button @click="go('/generator')">生成器</el-button>
        <el-button @click="go('/settings')">设置</el-button>
      </div>
    </section>
  </div>
</template>

<script setup>
import { useRouter } from 'vue-router'

const router = useRouter()

const updatedPages = [
  { title: '生成器页面', route: '/generator', note: '已改成统一 hero、工作区和构建状态布局。' },
  { title: '监听器页面', route: '/listeners', note: '已整理列表、stager 弹窗和新建监听器配置。' },
  { title: '受控端页面', route: '/clients', note: '已整理统计卡、列表区和右键操作入口。' }
]

const pendingPages = [
  { title: '客户端详情页', note: '已清理乱码文案，后续可继续统一视觉细节。' },
  { title: 'Client 子页面', note: '终端、文件、进程、插件分页面已完成主要乱码清理。' },
  { title: '零散文案清理', note: '建议后续继续检查旧按钮文案和风格混杂问题。' }
]

const go = (route) => {
  router.push(route)
}
</script>

<style scoped>
.review-shell {
  gap: 20px;
}

.review-meta {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.review-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 20px;
}

.review-card,
.quick-panel {
  padding: 24px;
}

.card-head {
  margin-bottom: 18px;
}

.card-head h3 {
  margin: 0;
}

.card-kicker {
  display: inline-block;
  margin-bottom: 8px;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.14em;
  color: var(--accent-strong);
}

.link-stack,
.todo-stack {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.review-link,
.todo-item {
  padding: 18px;
  border-radius: 20px;
  border: 1px solid var(--line-soft);
  background: var(--surface-soft);
}

.review-link {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  text-align: left;
  cursor: pointer;
}

.review-link strong,
.todo-item strong {
  display: block;
  margin-bottom: 6px;
  font-size: 16px;
}

.review-link p,
.todo-item p {
  margin: 0;
  color: var(--text-body);
  line-height: 1.6;
  font-size: 13px;
}

.review-link span {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-muted);
}

.quick-actions {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}

@media (max-width: 900px) {
  .review-grid {
    display: flex;
    flex-direction: column;
  }
}
</style>
