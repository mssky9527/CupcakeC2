<template>
  <div class="view-shell dashboard-shell">
    <section class="dashboard-grid">
      <article class="surface-card topology-card">
        <div class="panel-head">
          <div>
            <span class="panel-kicker">网络拓扑</span>
            <h3>节点拓扑与活动连接</h3>
          </div>
          <div class="chip">力导向布局</div>
        </div>
        <div class="topology-box">
          <v-chart class="chart-box" :option="topologyOption" autoresize />
        </div>
      </article>

      <div class="section-stack dashboard-side">
        <article class="surface-card side-card">
          <div class="panel-head panel-head--tight">
            <div>
              <span class="panel-kicker">系统分布</span>
              <h3>端点平台分布</h3>
            </div>
          </div>
          <div class="donut-wrap">
            <v-chart :option="osOption" autoresize />
          </div>
          <div class="os-legend">
            <div class="os-legend-item">
              <span class="os-dot" style="background:#111111"></span>
              <span>Windows</span>
              <strong>{{ osCounts.windows }}</strong>
            </div>
            <div class="os-legend-item">
              <span class="os-dot" style="background:#8c8c8c"></span>
              <span>Linux</span>
              <strong>{{ osCounts.linux }}</strong>
            </div>
          </div>
        </article>

        <article class="surface-card side-card">
          <div class="panel-head panel-head--tight">
            <div>
              <span class="panel-kicker">健康状态</span>
              <h3>主机资源压力</h3>
            </div>
          </div>

          <div class="meter-stack">
            <div class="meter-row">
              <div class="meter-copy">
                <span>CPU 负载</span>
                <strong>{{ stats.cpu_usage }}%</strong>
              </div>
              <el-progress :percentage="parseFloat(stats.cpu_usage) || 0" :show-text="false" color="#111111" :stroke-width="8" />
            </div>

            <div class="meter-row">
              <div class="meter-copy">
                <span>内存使用率</span>
                <strong>{{ stats.mem_usage }}%</strong>
              </div>
              <el-progress :percentage="parseFloat(stats.mem_usage) || 0" :show-text="false" color="#6a6a6a" :stroke-width="8" />
            </div>
          </div>
        </article>
      </div>
    </section>
  </div>
</template>

<script setup>
import { computed, onMounted, onUnmounted, ref } from 'vue'
import api from '../api/index'
import {
  Connection,
  Headset,
  Monitor
} from '@element-plus/icons-vue'

import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { GraphChart, PieChart } from 'echarts/charts'
import { GridComponent, LegendComponent, TitleComponent, TooltipComponent } from 'echarts/components'
import VChart from 'vue-echarts'

use([
  CanvasRenderer,
  PieChart,
  GraphChart,
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent
])

const stats = ref({
  cpu_usage: '0.0',
  mem_usage: '0.0',
  disk_usage: '0.0',
  uptime: 0,
  listener_count: 0,
  client_count: 0,
  online_count: 0,
  active_ports: [],
  locations: []
})

const histories = ref({
  cpu: [],
  mem: [],
  agents: [],
  listeners: []
})

const realLatency = ref(0)
const currentTime = ref('00:00:00')
const currentDate = ref('')

const summaryCards = computed(() => [
  {
    label: 'Online agents',
    value: stats.value.online_count,
    icon: Monitor,
    bg: 'rgba(17, 17, 17, 0.06)',
    color: '#111111'
  },
  {
    label: 'Active listeners',
    value: stats.value.listener_count,
    icon: Headset,
    bg: 'rgba(17, 17, 17, 0.08)',
    color: '#111111'
  },
  {
    label: 'Published routes',
    value: (stats.value.active_ports || []).length,
    icon: Connection,
    bg: 'rgba(31, 25, 20, 0.09)',
    color: '#1f1914'
  }
])

const updateTime = () => {
  const now = new Date()
  currentTime.value = now.toLocaleTimeString('en-GB', { hour12: false })
  currentDate.value = now.toLocaleDateString('en-GB', {
    weekday: 'short',
    year: 'numeric',
    month: 'short',
    day: 'numeric'
  })
}

const osCounts = computed(() => {
  const meta = { windows: 0, linux: 0 }
  const locations = stats.value.locations || []
  locations.forEach((agent) => {
    const os = (agent.os || '').toLowerCase()
    if (os.includes('win')) {
      meta.windows += 1
    } else {
      meta.linux += 1
    }
  })
  return meta
})

const osOption = computed(() => {
  const counts = osCounts.value
  const total = counts.windows + counts.linux

  const data = total > 0
    ? [
        { value: counts.windows, name: `Windows (${counts.windows})`, itemStyle: { color: '#111111' } },
        { value: counts.linux, name: `Linux (${counts.linux})`, itemStyle: { color: '#8c8c8c' } }
      ].filter(d => d.value > 0)
    : [
        { value: 1, name: '暂无端点', itemStyle: { color: '#e0e0e0' } }
      ]

  return {
    backgroundColor: 'transparent',
    tooltip: { trigger: 'item', formatter: total > 0 ? '{b}: {c} ({d}%)' : '' },
    series: [
      {
        type: 'pie',
        radius: ['62%', '88%'],
        center: ['50%', '50%'],
        label: { show: false },
        itemStyle: { borderColor: '#ffffff', borderWidth: 3 },
        data
      }
    ]
  }
})

const topologyOption = computed(() => {
  const nodes = [
    {
      name: 'Control Hub',
      x: 500,
      y: 300,
      fixed: true,
      symbol: 'roundRect',
      symbolSize: [150, 46],
      itemStyle: { color: '#111111', borderRadius: 12 },
      label: { show: true, color: '#ffffff', formatter: 'Control Hub', fontWeight: 'bold' }
    }
  ]

  const links = []
  const activePorts = stats.value.active_ports || []

  activePorts.forEach((port) => {
    const listenerName = `Listener ${port}`
    nodes.push({
      name: listenerName,
      symbol: 'roundRect',
      symbolSize: [104, 34],
      itemStyle: { color: '#3a3a3a', shadowBlur: 0, shadowColor: 'transparent' },
      label: { show: true, color: '#ffffff', fontSize: 10, formatter: `Port ${port}` }
    })
    links.push({ source: 'Control Hub', target: listenerName })
  })

  const locations = stats.value.locations || []
  locations.forEach((agent) => {
    const online = agent.status === 'active' || agent.status === 'online'
    const label = `${agent.name || 'Agent'}\n${agent.ip || 'unknown'}`
    nodes.push({
      name: agent.uuid,
      symbol: 'circle',
      symbolSize: online ? 22 : 16,
      itemStyle: {
        color: online ? '#fffaf2' : 'rgba(255, 250, 242, 0.38)',
        borderColor: online ? '#111111' : '#9a9a9a',
        borderWidth: 2,
        opacity: online ? 1 : 0.65
      },
      label: {
        show: true,
        position: 'bottom',
        color: online ? '#1f1914' : '#85786a',
        fontSize: 10,
        formatter: label,
        backgroundColor: online ? 'rgba(255, 255, 255, 0.82)' : 'transparent',
        padding: [2, 5],
        borderRadius: 4
      }
    })

    const parent = activePorts.length > 0 ? `Listener ${activePorts[0]}` : 'Control Hub'
    links.push({
      source: parent,
      target: agent.uuid,
      lineStyle: { type: online ? 'solid' : 'dashed', opacity: online ? 0.45 : 0.18 }
    })
  })

  return {
    backgroundColor: 'transparent',
    tooltip: {
      trigger: 'item',
      formatter: (params) => {
        if (params.dataType === 'node') {
          return `<b>${params.data.name}</b><br/>IP: ${params.data.ip || 'n/a'}`
        }
        return ''
      }
    },
    series: [
      {
        type: 'graph',
        layout: 'force',
        data: nodes,
        links,
        force: { repulsion: 620, edgeLength: 160, gravity: 0.08 },
      lineStyle: { color: '#111111', width: 2, curveness: 0.08 },
        roam: true,
        draggable: true,
        emphasis: { focus: 'adjacency', lineStyle: { width: 4, opacity: 0.82 } }
      }
    ]
  }
})

const fetchStats = async () => {
  const start = Date.now()
  try {
    const res = await api.get('/api/dashboard')
    stats.value = res.data
    realLatency.value = Date.now() - start

    const pushSample = (key, value) => {
      histories.value[key].push(value)
      if (histories.value[key].length > 30) {
        histories.value[key].shift()
      }
    }

    pushSample('cpu', parseFloat(stats.value.cpu_usage) || 0)
    pushSample('mem', parseFloat(stats.value.mem_usage) || 0)
    pushSample('agents', stats.value.online_count || 0)
    pushSample('listeners', stats.value.listener_count || 0)
  } catch (e) {
    console.error('API Error:', e)
  }
}

let ticker = null
let clockTicker = null

onMounted(() => {
  fetchStats()
  updateTime()
  ticker = setInterval(fetchStats, 10000)
  clockTicker = setInterval(updateTime, 1000)
})

onUnmounted(() => {
  clearInterval(ticker)
  clearInterval(clockTicker)
})
</script>

<style scoped>
.dashboard-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.55fr) minmax(320px, 0.9fr);
  gap: 20px;
  min-height: 620px;
}

.dashboard-hero {
  background:
    radial-gradient(circle at top right, rgba(17, 17, 17, 0.05), transparent 26%),
    linear-gradient(180deg, rgba(255, 255, 255, 0.96), rgba(250, 250, 250, 0.96));
}

.panel-head--tight {
  margin-bottom: 10px;
}

.topology-card,
.side-card {
  padding: 24px;
}

.topology-box {
  height: 540px;
}

.chart-box,
.donut-wrap :deep(canvas),
.meter-spark :deep(canvas) {
  width: 100%;
  height: 100%;
}

.dashboard-side {
  min-width: 0;
}

.donut-wrap {
  height: 280px;
}

.meter-stack {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.meter-row {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.meter-copy {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.meter-copy span {
  color: var(--text-body);
  font-size: 13px;
}

.meter-copy strong {
  font-size: 16px;
  letter-spacing: -0.03em;
}

.meter-spark {
  height: 28px;
}

.os-legend {
  display: flex;
  justify-content: center;
  gap: 24px;
  margin-top: 12px;
}

.os-legend-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-body);
}

.os-legend-item strong {
  font-size: 15px;
  color: #111;
}

.os-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  display: inline-block;
}

@media (max-width: 1220px) {
  .dashboard-grid {
    grid-template-columns: 1fr;
  }

  .topology-box {
    height: 420px;
  }
}

@media (max-width: 720px) {
  .topology-card,
  .side-card {
    padding: 18px;
  }

  .meter-copy {
    flex-direction: column;
    align-items: flex-start;
  }

  .topology-box {
    height: 340px;
  }
}
</style>
