# TenSunS CLI V3.0.0

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

默认 dry-run。格式根据扩展名识别，也可指定 --format txt|csv|json|jsonl。--service 可重复。--force 仅用于本地导出覆盖，绝不更新远端实例。完整输入校验、全量已有实例查询和报告创建成功后才允许远端写入。

TXT 六列：ServiceName|InstanceID|IP|Port|Tags|Meta，Tags 逗号分隔，Meta 为逗号分隔的 key=value，可留空但保留分隔符。CSV 固定首行 ServiceName,InstanceID,IP,Port,Tags,Meta，后两列为 JSON 数组和对象，可留空；查看不可信 CSV 时应按纯文本导入以避免公式注入。JSON 是六字段对象数组；JSONL 每行一个对象，字段 ID/name/address/port/tags/meta。Meta 值必须为字符串。TXT 遇到无法无损表达的记录拒绝导出，改用 JSON/CSV。输入名称和 ID 限制 ASCII 字符，ID 额外允许 @/，不允许首尾斜线和连续斜线。标签排序去重。空地址和端口 0 可解析，服务端兼容性待验证。

## 安全和错误处理

- 使用 HashMap 建立全量 ID 索引。相同 ID 且六字段一致为 unchanged，不同字段或多个匹配为 conflict。默认冲突整批不写入，--skip-conflicts 跳过冲突。
- 串行新增、写入失败即停止，不自动重试 POST、不删除、不自动回滚。按服务组集中回读，最多五次只读验证。
- JSONL 报告记录 planned、unchanged、conflict、submitting、accepted、verified、unverified、failed_or_uncertain、not_attempted，不记录 Tags/Meta 值。报告拒绝覆盖。中断后先查实际状态。
- 退出码 0 成功；1 错误或 apply 冲突；2 dry-run 冲突或 CLI 参数错误。
- 导出先查询完成并写同目录临时文件，默认不覆盖；拒绝已有符号链接或目录。避免同时修改输出路径。
- 重复检测基于快照而非原子事务，变更期间避免并发写入。健康检查完整定义、ACL 和节点归属未通过查询接口返回，导出不是完整灾备。新建默认关闭健康检查。暂不支持 PUT 更新或 DELETE 删除。

## GitHub Actions

main push、PR、手动运行及 v* tag 触发编译。Linux x64/ARM64、macOS Intel/Apple Silicon、Windows x64 各自原生 runner 执行 Rust 单测、release build、6001 实例模拟 API 集成测试，然后打包上传。所有平台通过后才允许 tag 发布。版本标签必须与 Cargo.toml 一致，例如 v3.0.0。

```bash
git tag v3.0.0
git push origin v3.0.0
```

首次 CI 结果以 Actions 为准；尚未在真实 TenSunS 环境联调。编译依赖、runner 可用性、实际系统兼容性需经 CI 和目标环境验证。
