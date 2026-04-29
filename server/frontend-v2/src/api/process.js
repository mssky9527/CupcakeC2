import request from '@/utils/request'

// ⚡️ FIXED: Use GET /api/processes/list
export function listProcesses(uuid) {
    return request({
        url: '/api/processes/list',
        method: 'get',
        params: { uuid }
    })
}

// ⚡️ FIXED: Use POST /api/processes/kill
export function killProcess(data) {
    // data: { uuid: '...', pid: 1234 }
    return request({
        url: '/api/processes/kill',
        method: 'post',
        data
    })
}
