# 智能报表实验

这个目录是“意图编译器”讨论的第一个可执行切片。它描述一个包含十张表的
B2B 商业领域，接受接近业务语义的报表意图，校验组合是否合法，并用普通
Forma 代码将合法意图逐步 lowering 为 SQLite SQL。

建议先阅读 [DOMAIN.md](DOMAIN.md)，其中定义了物理模型、语义身份、关系、
度量 grain 和第一批合法组合。

## 当前状态

- RFC 0180：伞 RFC 已接受，确定五阶段实验路线；
- RFC 0181：已完成，领域 capability 产生结构化 SQL AST，由统一 renderer
  负责 SQL 字符串和 quoting；
- RFC 0182：已完成，关系 catalog 与双向闭包 planner 根据 base grain 和目标
  entity 自动选择、合并并去重 join；
- RFC 0183：已完成，many-to-one 安全路径与 one-to-many fan-out 路径分离，
  Product 维度对 Order grain 的拒绝由关系证明产生；
- RFC 0184：已完成，意图支持 filter、显式排序、limit 和 render mode，并形成
  `SemanticPlan → RelationalPlan → SqlPlan` 三个 typed lowering 阶段；
- RFC 0185：已完成，成功结果发布 typed `ExecutionPlan`，边界模块将其编码为
  带版本、方言、只读声明和输出模式的稳定 JSON；
- RFC 0180：全部五个子阶段已经完成。
- RFC 0186：新的诊断伞 RFC 已接受；RFC 0187 已完成 message-first variadic
  `blame!` 与 `raise!`；RFC 0188 已完成普通 `report` BIF、Info/Warn/Error
  事件及 Error 成功边界；RFC 0189 已完成 `emit_info!`、`emit_warn!`、
  `emit_error!` 与 `fail!` 便利形式；RFC 0190 已完成领域库迁移，删除显式诊断
  数组，并证明普通 `Option`、数组组合子与 Host 事件足以覆盖当前本体实验；
- RFC 0186：全部四个子阶段已经完成。

## 文件

- `schema.sql`：十张 SQLite 表及确定性测试数据；
- `DOMAIN.md`：文字形式的本体和业务规则；
- `sql.forma`：最小 SQL AST 与 SQLite renderer；
- `relations.forma`：关系 catalog、可达性分析和 join 路径规划；
- `execution.forma`：Host-facing typed plan 与显式 wire encoding；
- `ontology.forma`：领域校验和 lowering；
- `valid.forma`：按月份、客户区域统计净收入；
- `valid-units.forma`：按月份、品类、SKU 统计销量；
- `invalid.forma`：一次暴露四个独立领域错误；
- `invalid-measures.forma`：拒绝多 measure 意图，不用任意 fallback 猜测依赖它的
  dimension 诊断；
- `valid-sql.forma`：导出生成的 SQL，供 SQLite 执行；
- `host-plan.forma`：模拟 Host shape 核对并输出 JSON plan；
- `net-revenue.sql`：手写参考查询。

## 运行

```sh
cargo run -p forma -- check examples/intelligent-reporting/valid.forma
cargo run -p forma -- run examples/intelligent-reporting/valid.forma
cargo run -p forma -- run examples/intelligent-reporting/valid-units.forma
cargo run -p forma -- run examples/intelligent-reporting/invalid.forma
```

生成的 SQL 已直接送入 SQLite。净收入结果为：

```text
2026-01|East|10000
2026-02|West|12000
```

销量结果为：

```text
2026-01|Keyboards|KB-1|2
2026-01|Mice|MS-1|1
2026-02|Mice|MS-1|4
```

## 当前证明了什么

- Forma 库可以承载一个小型、可执行的领域本体和业务规则；
- 面向 Code Agent 的意图只包含 measure 和 dimension，不暴露表、join、CTE、
  支付/退款语义或 SQL 语法；
- measure 和 dimension 是静态检查的 enum，而不是开放字符串；
- capability record 将领域概念与 lowering 函数绑定；
- 高阶 factory 可以表达通用、特定 measure 和暂不支持的 dimension 家族；
- 校验与 lowering 是同一过程：合法 dimension 产生 grouping requirement，非法
  组合产生诊断，不需要平行的 Boolean 兼容矩阵；
- 各 dimension lowerer 独立运行，一次编译可以报告四个错误；诊断是 Host 事件，
  不再是领域函数返回值；
- capability 不再拼接 SQL，标识符和字面量只由 renderer 转义；
- measure 只声明 base entity 和自身语义需要的 entity，dimension 只声明目标
  entity；关系 planner 从 catalog 计算二者之间的最小相关 edge 集合；
- 多个 dimension 共享的路径只产生一次 join；
- relation 带有 cardinality；planner 区分安全可达、需要 fan-out policy 和完全
  不可达，诊断仍指向原始 dimension；
- filter 本身也声明所需 entity，因此会参与同一关系规划；排序只能引用已经
  选择的 dimension，limit 与 render mode 保留在 typed plan 中；
- semantic、relational、SQL 三个中间计划都是普通 Forma 值和显式函数边界；
- 成功编译只发布无权限的 `Option(ExecutionPlan)`；Host 可以静态核对 shape，
  再接收显式版本化 JSON。失败 lowering 得到 `None`，任意 Error 事件同时阻止
  evaluation 被发布为成功；
- 失败结果不发布 SQL，成功结果可以直接被 SQLite 执行。

## RFC 0181 的边界发现

最自然的 SQL AST 是递归的，但当前 Forma 的递归类型元数据不能跨 legacy
module value boundary 发布，会报告：

```text
cyclic heap values cannot cross the legacy Value boundary
```

本实验没有退回字符串，而是采用可跨模块的非递归分层 AST：

```text
SqlAtom -> SqlTerm -> SqlScalar -> SqlSelectExpr
SelectBody -> Select（仅顶层包含 CTE）
```

它覆盖当前需要的列、常量、调用、二元表达式、聚合、CTE、join、filter、
group 和 order。任意深度表达式及嵌套 CTE 暂不支持；未来修复递归元数据发布
边界后，可以替换表示而不改变领域 capability 的职责。

## 尚未解决

这两轮伞 RFC 已完成，但它还不是通用查询规划器。当前 catalog 是有序、无代价
的有向关系集合，使用固定六轮闭包覆盖这个有界本体；它不在多条语义不同的
路径之间猜测。当前也
只有“保持 grain”与“fan-out”两级证明，尚未实现具体预聚合或 allocation
policy。后续阶段还需要：

- 预聚合与 allocation policy；
- 参数、drill 和更丰富的 render 意图；
- 参数、结果 schema 与 render plan 的一致性证明；
- 授权和 catalog 的显式 Context；
- provenance 穿过所有中间计划；
- CLI 失败输出完整呈现 Host 已收集的多条诊断；
- 递归 SQL AST 跨 legacy module value boundary。

诊断伞 RFC 还记录了一个不实现的远期扩展：由调用者显式使用
`call_with_diagnostics!(compiler(intent))`，在单个调用边界把子诊断重新数据化。
它将是 `interpreter!` 同级的受控内建语法，而不是普通函数或通用 effect
handler；只有嵌套意图编译器的真实需求足够明确时，才应另开 RFC 定义其类型和
Error 传播规则。

RFC 0190 已删除 `RequirementCompilation` 和诊断数组。当前实验没有证明需要
accumulation effect：可恢复的领域拒绝使用 `emit_error! + Option`，真正无法继续
的依赖链才使用 `raise!`。是否需要更细粒度恢复，应由新的真实场景重新举证。
