import request from '@/utils/request'

// 1. 列表必须是GET，路径必须是 /files/list
export function listFiles(params) {
    return request({
        url: '/api/files/list',
        method: 'get',
        params
    })
}

// 2. 读取必须是GET，路径必须是 /files/read
export function readFile(params) {
    return request({
        url: '/api/files/read',
        method: 'get',
        params
    })
}

// 3. 删除必须是POST，路径必须是 /files/delete
export function deleteFiles(data) {
    return request({
        url: '/api/files/delete',
        method: 'post',
        data
    })
}

// 4. 上传必须是 POST，路径 /api/files/upload
// 重要：不要手动设置 Content-Type: multipart/form-data。
// 必须由浏览器/axios 自动带上 boundary，否则服务端 FormFile 解析失败（表现为“无法上传但可下载”）。
export function uploadFile(data, onUploadProgress) {
    return request({
        url: '/api/files/upload',
        method: 'post',
        data,
        // 大文件走服务端分块转发，放宽超时
        timeout: 0,
        onUploadProgress,
        // 显式删除可能被拦截器/默认配置写上的 Content-Type
        transformRequest: [
            (body, headers) => {
                if (typeof FormData !== 'undefined' && body instanceof FormData) {
                    if (headers && typeof headers === 'object') {
                        delete headers['Content-Type']
                        delete headers['content-type']
                    }
                }
                return body
            },
        ],
    })
}
