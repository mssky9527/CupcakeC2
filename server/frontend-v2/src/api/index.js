import axios from 'axios'

export const request = axios.create({
    baseURL: import.meta.env.VITE_API_BASE_URL || '/',
    timeout: 30000 // Increased to 30s as compilation takes time
})

// Request Interceptor: Attach Auth Token
request.interceptors.request.use(config => {
    const token = localStorage.getItem('cupcake_token')
    if (token) {
        config.headers.Authorization = `Bearer ${token}`
    }
    return config
})

// Response Interceptor: Handle Unauthorized
request.interceptors.response.use(
    response => response,
    error => {
        if (error.response && error.response.status === 401) {
            localStorage.removeItem('cupcake_token')
            if (!window.location.hash.includes('/login')) {
                window.location.href = '#/login'
            }
        }
        return Promise.reject(error)
    }
)

export const generateClient = (data) => {
    return request.post('/api/generate', data, {
        responseType: 'blob',
        timeout: 0 // Compilation can take minutes on fresh release builds
    })
}

// 监听管理
export const getListeners = () => request.get('/api/listeners')
export const deleteClient = (uuid) => request.delete(`/api/clients/${uuid}`)

// 文件传输 (保持在 index.js)
export const fsDownload = (data, onDownloadProgress) => request.get('/api/files/download', {
    params: data,
    responseType: 'blob',
    onDownloadProgress
})

// 终端历史日志
export const getShellLogs = (uuid) => request.get(`/api/shell/${uuid}`)

export default request
