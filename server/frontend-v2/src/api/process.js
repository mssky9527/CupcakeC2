import request from '@/utils/request'

// Use GET /api/processes/list.
export function listProcesses(uuid) {
    return request({
        url: '/api/processes/list',
        method: 'get',
        params: { uuid }
    })
}

// Use POST /api/processes/kill.
export function killProcess(data) {
    return request({
        url: '/api/processes/kill',
        method: 'post',
        data
    })
}
