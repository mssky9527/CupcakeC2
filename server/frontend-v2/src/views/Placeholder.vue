<template>
  <div class="placeholder-container">
    <el-empty :description="$route.meta.title + ' 模块正在开发中...'" :image-size="200">
      <template #image>
        <el-icon :size="100" color="#30363d"><DataBoard /></el-icon>
      </template>
      <el-button type="primary" @click="$router.push('/')">返回仪表盘</el-button>
    </el-empty>
  </div>
</template>

<script setup>
import { DataBoard } from '@element-plus/icons-vue'
</script>

<style scoped>
.placeholder-container {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 50vh;
  margin-top: 50px;
}
:deep(.el-empty__description p) {
  color: var(--text-secondary);
  font-size: 16px;
}
</style>
