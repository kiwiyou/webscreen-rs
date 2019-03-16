const websocket = new WebSocket(`ws://${location.host}/ws/`)
const item = document.getElementById('view')
websocket.onmessage = ({ data }) => item.src = `data:image/jpeg;base64,${data}`