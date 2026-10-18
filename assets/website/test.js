'use strict';
const speed = new Speedtest();
// Exclude the optional IP/ISP lookup stage and all telemetry.
for (const [name, value] of Object.entries({test_order:'P_D_U',telemetry_level:0,getIp_ispInfo:false,url_dl:'garbage.php',url_ul:'empty.php',url_ping:'empty.php',xhr_ignoreErrors:0})) speed.setParameter(name, value);
const byId = id => document.getElementById(id);
let completed = false;
speed.onupdate = data => {
  for (const [id, key] of [['download','dlStatus'],['upload','ulStatus'],['ping','pingStatus'],['jitter','jitterStatus']]) byId(id).textContent = data[key] === '' || data[key] == null ? '—' : String(data[key]);
  const stages = {1:['正在测量下载速度',data.dlProgress],2:['正在测量延迟与抖动',data.pingProgress],3:['正在测量上传速度',data.ulProgress]};
  if (stages[data.testState]) { byId('status').textContent = stages[data.testState][0]; byId('progress').value = Number(stages[data.testState][1]) || 0; }
  completed = data.testState === 4;
};
speed.onend = aborted => {
  byId('start').disabled = false;
  byId('stop').disabled = true;
  byId('status').textContent = aborted ? '测试已停止或连接失败。可检查网络后重试。' : '测试完成。数值仅反映当前浏览器到本站的连接。';
  byId('progress').value = aborted ? 0 : 1;
};
byId('start').addEventListener('click', () => {
  byId('start').disabled = true; byId('stop').disabled = false; completed = false;
  byId('status').textContent = '正在建立测试连接…';
  try { speed.start(); } catch { speed.onend(true); }
});
byId('stop').addEventListener('click', () => speed.abort());
window.addEventListener('pagehide', () => { if (!completed) speed.abort(); });
