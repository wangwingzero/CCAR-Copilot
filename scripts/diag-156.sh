#!/usr/bin/env bash
# 诊断 156 出口带宽与 nginx 限流
set +e

echo '=== 网卡链路速度 (Mbps, 物理上限) ==='
for iface in $(ls /sys/class/net | grep -v lo); do
  speed=$(cat /sys/class/net/$iface/speed 2>/dev/null)
  echo "  $iface = ${speed} Mbps"
done
echo

echo '=== nginx 全局/站点 limit_rate / limit_conn / limit_req 配置 ==='
grep -rnE 'limit_rate|limit_conn|limit_req|sendfile_max_chunk' \
  /etc/nginx /www/server/panel/vhost /www/server/nginx/conf 2>/dev/null \
  | grep -v '#' | head -40
echo

echo '=== ccar-release 站点配置完整路径 ==='
find /etc/nginx /www/server/panel/vhost /www/server/nginx/conf -type f \( -name '*.conf' -o -name '*.confd' \) 2>/dev/null \
  | xargs grep -l -E '031986|hudawang|ccar-release|ccar-dl|ccar-update' 2>/dev/null
echo

echo '=== 5 秒内 ens17 实时吞吐 (bytes 发送累计差值) ==='
RX1=$(awk '/ens17:/ {print $2}' /proc/net/dev)
TX1=$(awk '/ens17:/ {print $10}' /proc/net/dev)
sleep 5
RX2=$(awk '/ens17:/ {print $2}' /proc/net/dev)
TX2=$(awk '/ens17:/ {print $10}' /proc/net/dev)
echo "  RX 速率 = $(( (RX2-RX1)/5 )) B/s = $(( (RX2-RX1)/5/1024 )) KB/s"
echo "  TX 速率 = $(( (TX2-TX1)/5 )) B/s = $(( (TX2-TX1)/5/1024 )) KB/s (这是发给用户的速度)"
echo

echo '=== nginx 主进程 + worker_processes / worker_connections ==='
nginx -V 2>&1 | tr ' ' '\n' | grep -E 'limit|stream|brotli|gzip' | head -10
echo
grep -E 'worker_processes|worker_connections|sendfile|tcp_nopush|aio' /etc/nginx/nginx.conf /www/server/nginx/conf/nginx.conf 2>/dev/null | grep -v '#' | head -20
echo

echo '=== 防火墙 / iptables rate-limit / tc qdisc 限流 ==='
iptables -L -n -v 2>/dev/null | grep -iE 'limit|rate' | head -10
tc qdisc show 2>/dev/null
echo

echo '=== 宝塔面板流量限制 (BT panel quota) ==='
ls -la /www/server/panel/data/sites/*.json 2>/dev/null | head -5
cat /www/server/panel/data/sites/*.json 2>/dev/null | python3 -c "import sys, json; data = sys.stdin.read(); print('总流量限制配置(若有):', 'speed_limit' in data or 'bandwidth' in data)" 2>/dev/null
echo

echo '=== 文件实际大小 ==='
ls -lh /www/wwwroot/ccar-release/downloads/CCAR*0.1.4* 2>/dev/null
echo

echo '=== 测试 156 本地 loopback 读取速度 (排除 nginx/磁盘瓶颈) ==='
FILE='/www/wwwroot/ccar-release/downloads/CCAR Copilot_0.1.4_x64-setup.exe'
if [ -f "$FILE" ]; then
  echo "loopback (跳过网络，纯磁盘+nginx): "
  curl -s -o /dev/null --max-time 10 -w "  Speed=%{speed_download}B/s Size=%{size_download}B Time=%{time_total}s\n" "http://127.0.0.1/downloads/CCAR%20Copilot_0.1.4_x64-setup.exe"
fi
echo
echo '诊断结束'
