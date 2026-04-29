import api from './index'

export function getActiveTunnels() {
    return api.get('/api/socks')
}

export function startTunnel(data) {
    // data: { uuid: "...", port: "1080" }
    return api.post('/api/socks/start', data)
}

export function stopTunnel(data) {
    // data: { port: "1080" }
    return api.post('/api/socks/stop', data)
}

export function deleteTunnel(data) {
    // data: { port: "1080" }
    return api.post('/api/tunnel/delete', data)
}
