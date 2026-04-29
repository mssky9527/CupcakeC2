import request from '@/utils/request'

// 1. 列表必须是 GET，路径必须是 /files/list
export function listFiles(params) {
    return request({
        url: '/api/files/list',
        method: 'get',
        params
    })
}

// 2. 读取必须是 GET，路径必须是 /files/read
export function readFile(params) {
    return request({
        url: '/api/files/read',
        method: 'get',
        params
    })
}

// 3. 删除必须是 POST，路径必须是 /files/delete
export function deleteFiles(data) {
    return request({
        url: '/api/files/delete',
        method: 'post',
        data
    })
}

// 4. 上传必须是 POST，路径必须是 /files/upload
export function uploadFile(data, onUploadProgress) {
    return request({
        url: '/api/files/upload',
        method: 'post',
        data,
        headers: { 'Content-Type': 'multipart/form-data' },
        onUploadProgress
    })
}
