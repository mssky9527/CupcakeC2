<template>
  <div class="dashboard-wrapper">
    <div class="commander-layout">
      
      <!-- Top Status Bar -->
      <div class="module command-status-bar">
        <div class="status-left">
          <div class="control-logo">
            <el-icon><MagicStick /></el-icon>
            <span class="logo-text">CUPCAKE <span class="v-tag">控制中枢 V4</span></span>
          </div>
          <div class="divider"></div>
          <div class="meta-slot">
            <span class="s-label">基础设施状态</span>
            <span class="s-val">系统运行正常</span>
          </div>
          <div class="meta-slot">
            <span class="s-label">网络延时</span>
            <span class="s-val purple-glow">{{ realLatency }} 毫秒</span>
          </div>
        </div>
        <div class="status-right">
          <div class="clock-display">
            <span class="date-part">{{ currentDate }}</span>
            <span class="time-part">{{ currentTime }}</span>
          </div>
        </div>
      </div>

      <div class="main-deck">
        <!-- Center: Strategic Topology Map -->
        <div class="module glass-panel topology-deck">
          <div class="deck-header">
            <div class="h-title">
              <el-icon class="purple-text"><Aim /></el-icon>
              <span>内网战役拓扑节点图</span>
            </div>
            <div class="scan-tag">
              <span class="scan-line"></span>
              实时扫描中
            </div>
          </div>
          <div class="topology-container">
            <v-chart class="chart-box" :option="topologyOption" autoresize />
          </div>
        </div>

        <!-- Right: Tactical Intelligence Sidebar -->
        <div class="tactical-sidebar">
          
          <div class="module glass-panel intel-card">
            <div class="i-label">在线受控端</div>
            <div class="i-body">
              <span class="i-val">{{ stats.online_count }}</span>
              <div class="spark-box">
                <v-chart :option="miniSparkOption('#7c3aed', histories.agents)" autoresize />
              </div>
            </div>
          </div>

          <div class="module glass-panel intel-card highlighting">
            <div class="i-label">活跃监听器</div>
            <div class="i-body">
              <span class="i-val">{{ stats.listener_count }}</span>
              <div class="spark-box">
                <v-chart :option="miniSparkOption('#06b6d4', histories.listeners)" autoresize />
              </div>
            </div>
          </div>

          <div class="module glass-panel os-donut-card">
            <div class="i-label">操作系统分布</div>
            <div class="donut-box">
              <v-chart :option="osOption" autoresize />
            </div>
          </div>

          <div class="module glass-panel system-health-box">
            <div class="i-label">核心系统负载</div>
            <div class="health-meters">
              <div class="meter-item">
                <div class="m-info"><span>CPU</span> <span>{{ stats.cpu_usage }}%</span></div>
                <el-progress :percentage="parseFloat(stats.cpu_usage) || 0" :show-text="false" color="#7c3aed" stroke-width="6" />
                <div class="mini-trend">
                   <v-chart :option="miniSparkOption('#7c3aed', histories.cpu, 20)" autoresize style="height: 20px" />
                </div>
              </div>
              <div class="meter-item">
                <div class="m-info"><span>内存占用</span> <span>{{ stats.mem_usage }}%</span></div>
                <el-progress :percentage="parseFloat(stats.mem_usage) || 0" :show-text="false" color="#fb7185" stroke-width="6" />
                <div class="mini-trend">
                   <v-chart :option="miniSparkOption('#fb7185', histories.mem, 20)" autoresize style="height: 20px" />
                </div>
              </div>
            </div>
          </div>

        </div>
      </div>

    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted, computed } from 'vue'
import api from '../api/index'
import { 
  Monitor, Headset, Cpu, MagicStick, 
  Share, Connection, Aim, Histogram
} from '@element-plus/icons-vue'

// Echarts Core & Components
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart, PieChart, GraphChart } from 'echarts/charts'
import {
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent
} from 'echarts/components'
import VChart from 'vue-echarts'

use([
  CanvasRenderer, LineChart, PieChart, GraphChart,
  TitleComponent, TooltipComponent, GridComponent, LegendComponent
])

const stats = ref({
  cpu_usage: "0.0", mem_usage: "0.0", disk_usage: "0.0",
  uptime: 0, listener_count: 0, client_count: 0,
  online_count: 0, active_ports: [], locations: []
})

const histories = ref({
  cpu: [], mem: [], agents: [], listeners: []
})
const realLatency = ref(0)


// Clock Logic
const currentTime = ref('00:00:00')
const currentDate = ref('')
const updateTime = () => {
  const now = new Date()
  currentTime.value = now.toLocaleTimeString('zh-CN', { hour12: false })
  currentDate.value = now.toLocaleDateString('zh-CN', { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' })
}

// Mini Sparkline Generator
const miniSparkOption = (color, data, height = 40) => ({
  backgroundColor: 'transparent',
  grid: { top: 2, bottom: 2, left: 0, right: 0 },
  xAxis: { type: 'category', show: false },
  yAxis: { type: 'value', show: false, min: 'dataMin', max: 'dataMax' },
  series: [{
    type: 'line', smooth: true, symbol: 'none',
    data: data && data.length > 0 ? data : [0, 0, 0],
    lineStyle: { color: color, width: 2 },
    areaStyle: { 
      color: {
        type: 'linear', x: 0, y: 0, x2: 0, y2: 1,
        colorStops: [{ offset: 0, color: color }, { offset: 1, color: 'transparent' }]
      },
      opacity: 0.1 
    }
  }]
})

// OS Distribution Ring
const osOption = computed(() => {
  const meta = { win: 0, lin: 0 }
  const locations = stats.value.locations || []
  locations.forEach(a => {
    const os = (a.os || '').toLowerCase()
    if (os.includes('win')) meta.win++
    else meta.lin++
  })

  return {
    backgroundColor: 'transparent',
    tooltip: { trigger: 'item' },
    series: [{
      type: 'pie', radius: ['65%', '90%'], center: ['50%', '50%'],
      itemStyle: { borderRadius: 4, borderColor: '#fff', borderWidth: 2 },
      label: { show: false },
      data: [
        { value: meta.win || 1, name: 'Windows', itemStyle: { color: '#7c3aed' } },
        { value: meta.lin || 1, name: 'Linux', itemStyle: { color: '#06b6d4' } },
      ]
    }]
  }
})

// Strategic Topology Mapping (Powered by Real Data)
const topologyOption = computed(() => {
  const nodes = [
    { 
      name: 'NEXUS_HUB', x: 500, y: 300, fixed: true, 
      symbol: 'rect', symbolSize: [130, 40], 
      itemStyle: { color: '#1e1b4b', borderRadius: 8 }, 
      label: { show: true, color: '#fff', formatter: '控制中枢', fontWeight: 'bold' } 
    }
  ]
  const links = []
  
  // 1. Add Listeners
  const activePorts = stats.value.active_ports || []
  activePorts.forEach((port, idx) => {
    const lName = `LSTN:${port}`
    nodes.push({ 
      name: lName, symbol: 'roundRect', symbolSize: [90, 32], 
      itemStyle: { color: '#7c3aed', shadowBlur: 10, shadowColor: '#7c3aed66' },
      label: { show: true, color: '#fff', fontSize: 10, formatter: `端口 ${port}` }
    })
    links.push({ source: 'NEXUS_HUB', target: lName })
  })

  // 2. Add Real Agents (Live + Historical)
  const locations = stats.value.locations || []
  locations.forEach((agent) => {
    const isOnline = agent.status === 'active' || agent.status === 'online'
    const aLabel = `${agent.name}\n${agent.ip}`
    
    nodes.push({ 
      name: agent.uuid, 
      symbol: 'circle', 
      symbolSize: isOnline ? 22 : 16, 
      itemStyle: { 
        color: isOnline ? '#fff' : 'rgba(255,255,255,0.05)', 
        borderColor: isOnline ? '#7c3aed' : '#94a3b8', 
        borderWidth: 2,
        opacity: isOnline ? 1 : 0.4
      },
      label: { 
        show: true, position: 'bottom', 
        color: isOnline ? '#1e1b4b' : '#94a3b8', 
        fontSize: 10, 
        formatter: aLabel,
        backgroundColor: isOnline ? 'rgba(255,255,255,0.8)' : 'transparent',
        padding: [2, 4],
        borderRadius: 4
      }
    })

    // Logic: Connect to the first listener if exists, else HUB
    const parent = activePorts.length > 0 ? `LSTN:${activePorts[0]}` : 'NEXUS_HUB'
    links.push({ 
      source: parent, 
      target: agent.uuid,
      lineStyle: { type: isOnline ? 'solid' : 'dashed', opacity: isOnline ? 0.3 : 0.1 }
    })
  })

  return {
    backgroundColor: 'transparent',
    tooltip: { 
      trigger: 'item',
      formatter: (params) => {
        if (params.dataType === 'node') {
          return `<b>主机:</b> ${params.data.name}<br/><b>IP:</b> ${params.data.ip || '---'}`
        }
        return ''
      }
    },
    series: [{
      type: 'graph', layout: 'force',
      data: nodes, links: links,
      force: { repulsion: 600, edgeLength: 160, gravity: 0.1 },
      lineStyle: { color: '#7c3aed', width: 2, curveness: 0.1 },
      roam: true, draggable: true,
      emphasis: { focus: 'adjacency', lineStyle: { width: 4, opacity: 0.8 } }
    }]
  }
})

const fetchStats = async () => {
  const start = Date.now()
  try {
    const res = await api.get('/api/dashboard')
    stats.value = res.data
    realLatency.value = Date.now() - start
    
    // Record History Samples
    const pushSample = (key, val) => {
      histories.value[key].push(val)
      if (histories.value[key].length > 30) histories.value[key].shift()
    }
    
    pushSample('cpu', parseFloat(stats.value.cpu_usage))
    pushSample('mem', parseFloat(stats.value.mem_usage))
    pushSample('agents', stats.value.online_count)
    pushSample('listeners', stats.value.listener_count)

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
.dashboard-wrapper {
  padding: 15px;
  height: 100%;
  animation: slideUp 0.6s cubic-bezier(0.23, 1, 0.32, 1);
}

@keyframes slideUp {
  from { opacity: 0; transform: translateY(30px); }
  to { opacity: 1; transform: translateY(0); }
}

.commander-layout {
  display: flex;
  flex-direction: column;
  gap: 20px;
  max-width: 1700px;
  margin: 0 auto;
}

/* Status Bar */
.command-status-bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 25px;
  background: white;
  border-radius: 16px;
  box-shadow: 0 4px 20px rgba(0,0,0,0.03);
}

.status-left { display: flex; align-items: center; gap: 24px; }
.control-logo { display: flex; align-items: center; gap: 10px; }
.control-logo .el-icon { font-size: 20px; color: #7c3aed; }
.logo-text { font-weight: 900; color: #1e1b4b; font-size: 15px; }
.v-tag { color: #7c3aed; font-size: 11px; opacity: 0.8; }

.divider { width: 1px; height: 16px; background: rgba(0,0,0,0.1); }
.meta-slot { display: flex; flex-direction: column; }
.s-label { font-size: 8px; font-weight: 900; color: #cbd5e1; letter-spacing: 0.5px; }
.s-val { font-size: 11px; font-weight: 800; color: #1e1b4b; }
.purple-glow { color: #7c3aed; text-shadow: 0 0 8px rgba(124, 58, 237, 0.3); }

.clock-display { 
  display: flex; flex-direction: column; align-items: flex-end; 
  font-family: 'JetBrains Mono', monospace; 
}
.date-part { font-size: 9px; font-weight: 700; color: #94a3b8; }
.time-part { font-size: 22px; font-weight: 800; color: #1e1b4b; line-height: 1; }

.main-deck {
  display: flex;
  gap: 20px;
}

/* Topology Deck */
.topology-deck {
  flex: 1;
  height: 720px;
  padding: 0 !important;
  background: white !important;
  position: relative;
  overflow: hidden;
  border: 1px solid rgba(124,58,237,0.05);
}

.topology-deck::before {
  content: ''; position: absolute; inset: 0;
  background-image: 
    linear-gradient(rgba(124,58,237,0.03) 1px, transparent 1px),
    linear-gradient(90deg, rgba(124,58,237,0.03) 1px, transparent 1px);
  background-size: 40px 40px;
}

.deck-header {
  position: absolute; top: 20px; left: 25px; right: 25px;
  display: flex; justify-content: space-between; align-items: center;
  z-index: 10; pointer-events: none;
}
.h-title { display: flex; align-items: center; gap: 10px; font-weight: 900; color: #1e1b4b; font-size: 13px; }
.scan-tag {
  font-size: 9px; font-weight: 900; color: #22c55e;
  border: 1px solid rgba(34, 197, 94, 0.2);
  padding: 4px 10px; border-radius: 6px; background: rgba(34, 197, 94, 0.05);
  display: flex; align-items: center; gap: 8px;
}
.scan-line { width: 6px; height: 6px; background: #22c55e; border-radius: 50%; box-shadow: 0 0 8px #22c55e; animation: blink 1.2s infinite; }
@keyframes blink { 0%, 100% { opacity: 1; } 50% { opacity: 0.3; } }

.topology-container { width: 100%; height: 100%; }

/* Tactical Sidebar */
.tactical-sidebar {
  width: 320px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.glass-panel {
  background: rgba(255, 255, 255, 0.85);
  backdrop-filter: blur(20px);
  border: 1px solid rgba(124, 58, 237, 0.08);
  border-radius: 20px;
  padding: 24px;
}

.intel-card { position: relative; }
.i-label { font-size: 9px; font-weight: 900; color: #94a3b8; letter-spacing: 1.2px; margin-bottom: 8px; }
.i-body { display: flex; align-items: flex-end; justify-content: space-between; }
.i-val { font-family: 'JetBrains Mono'; font-size: 42px; font-weight: 900; color: #1e1b4b; line-height: 1; letter-spacing: -2px; }
.spark-box { width: 100px; height: 40px; opacity: 0.6; }

.os-donut-card { height: 200px; }
.donut-box { height: 160px; width: 100%; margin-top: -10px; }

.health-meters { display: flex; flex-direction: column; gap: 18px; margin-top: 5px; }
.meter-item { display: flex; flex-direction: column; gap: 6px; }
.m-info { display: flex; justify-content: space-between; font-size: 10px; font-weight: 900; color: #64748b; }
.mini-trend { height: 20px; margin-top: 4px; opacity: 0.4; }

@media (max-width: 1400px) {
  .main-deck { flex-direction: column; }
  .topology-deck { height: 500px; }
  .tactical-sidebar { width: 100%; flex-direction: row; }
  .intel-card, .os-donut-card, .system-health-box { flex: 1; height: auto; }
}
</style>
