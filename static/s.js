const websocket = new WebSocket(`ws://${location.host}/ws/`)
const item = document.getElementById('view')
websocket.onmessage = ({ data }) => item.src = `data:image/png;base64,${data}`