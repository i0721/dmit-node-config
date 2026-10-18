'use strict';
let token = '';
let generation = 0;
const element = id => document.getElementById(id);
const message = text => { element('message').textContent = text; };
function clear() {
  generation++;
  token = '';
  element('token').value = '';
  element('content').hidden = true;
  element('login').hidden = false;
  element('nodes').replaceChildren();
  element('formats').replaceChildren();
  element('release').textContent = '';
}
async function authorizedFetch(path) {
  const current = generation;
  const response = await fetch(path, {headers: {Authorization: `Bearer ${token}`}, cache: 'no-store', credentials: 'omit', redirect: 'error', referrerPolicy: 'no-referrer'});
  if (current !== generation) throw new DOMException('操作已取消', 'AbortError');
  if (!response.ok) {
    if (response.status === 401) { clear(); message('访问凭据无效或已撤销，请重新输入。'); }
    throw new Error('访问未获授权或服务暂不可用，请核对 Token 后重试。');
  }
  return response;
}
element('open').addEventListener('click', async () => {
  const current = ++generation;
  token = element('token').value;
  element('token').value = '';
  try {
    const data = await (await authorizedFetch('/nodes/api')).json();
    if (current !== generation) return;
    element('release').textContent = data.release;
    element('nodes').replaceChildren();
    for (const profile of data.clients.profiles) {
      const item = document.createElement('li');
      const server = profile.outbound.settings.vnext[0];
      item.textContent = `${profile.name} · ${server.address}:${server.port} · 权重 ${profile.weight.numerator}/${profile.weight.denominator}`;
      element('nodes').append(item);
    }
    element('formats').replaceChildren();
    for (const [format, label, filename] of [['mihomo', 'Mihomo', 'proxies.yaml'], ['xray', 'Xray', 'profiles.json']]) {
      const row = document.createElement('p');
      const copy = document.createElement('button');
      copy.textContent = `复制 ${label} 订阅地址`;
      copy.addEventListener('click', async () => {
        const active = generation;
        try {
          // Revalidate on every click: do not copy a revoked cached address.
          const current = await (await authorizedFetch('/nodes/api')).json();
          if (active !== generation) return;
          const address = new URL(current.subscriptions[format], location.origin).href;
          try {
            await navigator.clipboard.writeText(address);
          } catch {
            if (active !== generation) return;
            let manual = row.querySelector('textarea');
            if (!manual) {
              manual = document.createElement('textarea');
              manual.readOnly = true;
              manual.className = 'subscription-address';
              manual.setAttribute('aria-label', `手动复制 ${label} 订阅地址`);
              row.append(manual);
            }
            manual.value = address; manual.focus(); manual.select();
            message('浏览器未允许自动复制。请复制已选中的地址，并仅保存在自己的客户端中。');
            return;
          }
          if (active !== generation) return;
          row.querySelector('textarea')?.remove();
          message('订阅地址已复制，请仅保存在自己的客户端中。');
        } catch (error) { if (error.name !== 'AbortError') message(error.message); }
      });
      const download = document.createElement('button');
      download.textContent = `下载 ${label} 配置`;
      download.addEventListener('click', async () => {
        const active = generation;
        try {
          const response = await authorizedFetch(`/nodes/sub/${format}`);
          const blob = await response.blob();
          if (active !== generation) return;
          const url = URL.createObjectURL(blob);
          const link = document.createElement('a');
          link.href = url; link.download = filename; link.click();
          setTimeout(() => URL.revokeObjectURL(url), 1000);
          message('配置已下载，请安全保存。');
        } catch (error) { if (error.name !== 'AbortError') message(error.message); }
      });
      row.append(copy, download); element('formats').append(row);
    }
    element('login').hidden = true; element('content').hidden = false;
    message('验证通过。订阅更新时会读取当前成功部署的配置。');
  } catch (error) { if (current === generation) { clear(); message(error.message); } }
});
element('logout').addEventListener('click', () => { clear(); message('本页凭据已清除。'); });
window.addEventListener('pagehide', clear);
