import { createRouter, createWebHashHistory } from 'vue-router'
import MainLayout from '../layout/MainLayout.vue'

const routes = [
    {
        path: '/',
        component: MainLayout,
        redirect: '/dashboard',
        children: [
            {
                path: 'dashboard',
                name: 'Dashboard',
                component: () => import('../views/Dashboard.vue'),
                meta: { title: '仪表盘' }
            },
            {
                path: 'clients',
                name: 'Clients',
                component: () => import('../views/ClientManager.vue'),
                meta: { title: '客户端管理' }
            },
            {
                path: 'listeners',
                name: 'Listeners',
                component: () => import('../views/ListenerManager.vue'),
                meta: { title: '监听管理' }
            },
            {
                path: 'tunnels',
                name: 'Tunnels',
                component: () => import('../views/server/TunnelManager.vue'),
                meta: { title: '隧道管理' }
            },
            {
                path: 'generator',
                name: 'Generator',
                component: () => import('../views/PayloadGenerator.vue'),
                meta: { title: 'C2生成' }
            },
            {
                path: 'domain',
                name: 'Domain',
                component: () => import('../views/DomainScanner.vue'),
                meta: { title: '插件管理' }
            },
            {
                path: 'settings',
                name: 'Settings',
                component: () => import('../views/Settings.vue'),
                meta: { title: '系统设置' }
            },
            {
                path: 'client/:id',
                name: 'ClientDetail',
                component: () => import('../views/ClientDetail.vue'),
                redirect: (to) => ({ name: 'ClientTerminals', params: { id: to.params.id } }),
                meta: { title: '客户端详情' },
                children: [
                    {
                        path: 'terminals',
                        name: 'ClientTerminals',
                        component: () => import('../components/TerminalTabs.vue'),
                        meta: { title: '终端' }
                    },
                    {
                        path: 'files',
                        name: 'ClientFiles',
                        component: () => import('../views/client/FileManager.vue'),
                        meta: { title: '文件管理' }
                    },
                    {
                        path: 'tunnels',
                        name: 'ClientTunnels',
                        component: () => import('../views/client/TunnelManager.vue'),
                        meta: { title: '隧道管理' }
                    },
                    {
                        path: 'processes',
                        name: 'ClientProcesses',
                        component: () => import('../views/client/ProcessManager.vue'),
                        meta: { title: '进程管理' }
                    },
                    {
                        path: 'plugins',
                        name: 'ClientPlugins',
                        component: () => import('../views/client/PluginManager.vue'),
                        meta: { title: '插件管理' }
                    }
                ]
            }
        ]
    },
    {
        path: '/login',
        name: 'Login',
        component: () => import('../views/Login.vue'),
        meta: { title: '登录' }
    }
]

const router = createRouter({
    history: createWebHashHistory(),
    routes
})

// Authentication Guard
router.beforeEach((to, from, next) => {
    const token = localStorage.getItem('cupcake_token')
    if (to.name !== 'Login' && !token) {
        next({ name: 'Login' })
    } else if (to.name === 'Login' && token) {
        next({ name: 'Dashboard' })
    } else {
        next()
    }
})

export default router
