# TenSunS CLI V4.2.1

Rust 重构版，通过 TenSunS Web API 批量管理自定义实例。运行无需 Bash、curl、jq 或 Python；Python 仅用于 CI 集成测试及打包。

## 安装与编译

从 GitHub Actions 下载对应系统的压缩包，或在版本标签发布后从 Releases 下载。Linux/macOS 使用 tar.gz，Windows 使用 zip；每个压缩包附独立 SHA256 校验文件。Linux GNU 目标依赖兼容的 glibc，不保证旧版 Linux 兼容。macOS/Windows 未配置代码签名、公证或安装器。

本地编译需要 Rust stable 和 crates.io 访问：
```bash
cargo build --release
cargo test --all-targets
python3 tests/test_mock.py target/release/tensuns
```
Windows 可执行文件为 target/release/tensuns.exe。首次构建生成 Cargo.lock；建议将生成并审核的锁文件提交。CI 在未提交锁文件时先生成一次，并让所有平台共用此锁文件。Rust 1.85 为源码最低目标，未锁定依赖可能要求更高版本。

## Web 控制台（V4.2）

```bash
tensuns --url https://tensuns.example.com serve --host 127.0.0.1 --port 8080
```

打开终端打印的 URL。默认仅允许 loopback 地址，不提供公网/局域网服务模式。Web 登录页可直接编辑 TenSunS Web URL 和用户名，启动参数只是默认值；也可选择已保存连接。新 URL + 用户名通过认证且目录查询成功后自动保存为独立连接，不会改变旧连接 ID 或历史报告。密码由浏览器输入，不使用 CLI 的 TENSUNS_TOKEN。连接管理可保存多个 URL 和用户名，退出后选择连接重新登录。登录后支持服务组、实例搜索与分页、单条新增、五格式文件导入预览、冲突策略、明确确认后执行及回读、全量/指定服务组导出、任务报告下载。单条新增、批量导入和同步预览使用独立页面模式，切换时清理旧预览及确认状态。前端 HTML/CSS/JS 嵌入同一 Rust 可执行文件，无 Node.js、CDN 或外置资源。

Web 每个进程仅一个本地活动会话，登录成功可替换旧会话。会话凭据只在浏览器 JS 内存与服务端内存，刷新页面需重新登录，登出清理。会话有效期 1 小时；预览有效期 10 分钟。所有 API POST 检查 Origin、Host、JSON 类型与会话头（登录除外），不启用 CORS。浏览器连接本地 HTTP，后端建议 HTTPS。批量导入最大 16 MiB HTTP 请求、10000 条实例；每次操作串行执行，应用前重新获取全量配置及比较预览计划，写入不重试。连接配置、任务状态和报告保存在 --report-dir（默认 tensuns-web-reports），每个连接独立 UUID 目录；报告可在重新登录后下载。运行时轮询显示进度，进程意外终止的任务标记 interrupted。服务端启动时固定报告目录；请使用用户独占的本地目录。大批量请求可能需要较长时间，页面显示执行状态，勿重复提交。报告不保证能恢复完整 Consul 配置。

尚未实现多人共享、RBAC、后台任务自动恢复、批量更新删除、健康检查完整恢复、Docker/Helm。请勿通过反向代理暴露此本地控制台。

## V4.2 配置快照、Diff 与跨环境新增同步

1. 在来源 TenSunS 连接登录，进入「配置快照」，创建全量或指定服务组的快照。
2. 退出并登录目标连接，在「差异比较 / 同步」选择来源快照和当前连接，先执行只读比较。也可选择另一份全量历史快照作为目标。
3. Diff 状态：missing_in_target（目标缺失）、changed（字段差异）、target_only（目标独有）、ambiguous（ID 歧义）、unchanged（一致）。比较 ID、服务组、地址、端口、Tags、Meta。选择服务组时，仍检测其他目标服务组的相同 ID；目标独有仅在选择范围内统计。
4. 点击生成新增同步预览，检查明确显示的来源、当前目标、冲突和计划。确认后只创建缺失 ID，目标冲突默认整批阻止，可明确跳过；不会删除目标独有或修改冲突项。重新查询目标验证计划，回读及持久化报告保留来源快照 ID。
5. 同步使用本地快照而非两个同时在线的连接；快照保存时点为逐组查询结果，不是 Consul 原子事务。快照可下载基础 JSON，历史对比的目标必须为全量快照。当前为单 OS 用户工具，登录者可使用工作目录内所有本地快照；请保护目录及备份。
6. CLI 也支持 `tensuns diff instances.xlsx --changes-only`，输出只读 JSON 差异和全量计数。CLI 跨环境新增同步可以先用来源 URL export，再用目标 URL import --dry-run / --apply。

V4.0 旧根目录报告不会自动迁移到按连接的 V4.1+ 工作区；原 JSONL 文件保留，可手动查阅。V4.2 不提供批量更新、删除、自动覆盖、分布式锁或完整 Consul 灾备。

## 连接设置

默认地址为 http://127.0.0.1:1026。请用真实 TenSunS Web 地址覆盖：
```bash
export TENSUNS_URL='https://tensuns.example.com'
export TENSUNS_USER='admin'
export TENSUNS_TIMEOUT=120
```
PowerShell 使用 $env:TENSUNS_URL = 'https://tensuns.example.com'。亦支持 --url、--user、--timeout。交互密码不回显；非交互可通过可信 stdin 提供单行密码或注入 TENSUNS_TOKEN（可含 Bearer 前缀）。HTTP 明文传输凭据，建议 HTTPS。默认验证证书且不跟随重定向；代理环境请正确设置 NO_PROXY。不要提交真实凭据、内网配置和导入报告。

## 命令

```bash
tensuns list
tensuns list --service snmp_exporter
tensuns export all.json
tensuns export selected.csv --service snmp_exporter --service node_exporter
tensuns export all.json --force
tensuns validate examples/instances.txt
tensuns import examples/instances.txt --dry-run --report plan.jsonl
tensuns import examples/instances.txt --apply --report result.jsonl
tensuns import examples/instances.csv --apply --skip-conflicts
tensuns import examples/instances.jsonl --dry-run
tensuns examples/instances.txt --dry-run
```

默认 dry-run。格式根据扩展名识别，也可指定 --format txt|csv|json|jsonl|xlsx。--service 可重复。--force 仅用于本地导出覆盖，绝不更新远端实例。完整输入校验、全量已有实例查询和报告创建成功后才允许远端写入。

TXT 六列：ServiceName|InstanceID|IP|Port|Tags|Meta，Tags 逗号分隔，Meta 为逗号分隔的 key=value，可留空但保留分隔符。CSV 固定首行 ServiceName,InstanceID,IP,Port,Tags,Meta，后两列为 JSON 数组和对象，可留空；查看不可信 CSV 时应按纯文本导入以避免公式注入。JSON 是六字段对象数组；JSONL 每行一个对象，字段 ID/name/address/port/tags/meta。Meta 值必须为字符串。TXT 遇到无法无损表达的记录拒绝导出，改用 JSON/CSV。输入名称和 ID 限制 ASCII 字符，ID 额外允许 @/，不允许首尾斜线和连续斜线。标签排序去重。空地址和端口 0 可解析，服务端兼容性待验证。

XLSX 使用单工作表、同 CSV 六列，Tags/Meta 为 JSON 字符串；不接受公式、最多 10000 条，ZIP 解压大小限制。可在 Web 下载 XLSX 模板，也可用 CLI 导出 XLSX。Web 支持持久化连接与任务记录，但一次只有一个活动会话，任务串行执行，不能暂停或自动恢复。请不要让多个进程共用工作目录。

## 安全和错误处理

- 使用 HashMap 建立全量 ID 索引。相同 ID 且六字段一致为 unchanged，不同字段或多个匹配为 conflict。默认冲突整批不写入，--skip-conflicts 跳过冲突。
- 串行新增、写入失败即停止，不自动重试 POST、不删除、不自动回滚。按服务组集中回读，最多五次只读验证。
- JSONL 报告记录 planned、unchanged、conflict、submitting、accepted、verified、unverified、failed_or_uncertain、not_attempted，不记录 Tags/Meta 值。报告拒绝覆盖。中断后先查实际状态。
- 退出码 0 成功；1 错误或 apply 冲突；2 dry-run 冲突或 CLI 参数错误。
- 导出先查询完成并写同目录临时文件，默认不覆盖；拒绝已有符号链接或目录。避免同时修改输出路径。
- 重复检测基于快照而非原子事务，变更期间避免并发写入。健康检查完整定义、ACL 和节点归属未通过查询接口返回，导出不是完整灾备。新建默认关闭健康检查。暂不支持 PUT 更新或 DELETE 删除。

## GitHub Actions

main push、PR、手动运行及 v* tag 触发编译。Linux x64/ARM64、macOS Intel/Apple Silicon、Windows x64 各自原生 runner 执行 Rust 单测、release build、6001 实例模拟 API 集成测试，然后打包上传。所有平台通过后才允许 tag 发布。版本标签必须与 Cargo.toml 一致，例如 v4.2.1。

```bash
git tag v4.2.1
git push origin v4.2.1
```

首次 CI 结果以 Actions 为准；尚未在真实 TenSunS 环境联调。编译依赖、runner 可用性、实际系统兼容性需经 CI 和目标环境验证。
