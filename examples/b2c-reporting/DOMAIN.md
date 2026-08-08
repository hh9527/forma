# B2C 分析模型

这个模型刻意不同于 B2B 示例。它包含十二个物理实体：Consumer、Household、
Address、Region、Session、Cart、CartItem、Order、OrderItem、Product、Category 和
Campaign。

第一批语义能力包括：

- `PurchaseRevenue`：自然 grain 是 Order；
- `SessionConversions`：自然 grain 是 Session；
- `PurchaseMonth`、`ConsumerRegion` 与 `AcquisitionCampaign`：从 Order grain
  可以通过 many-to-one 路径安全取得；
- `ProductCategory`：从 Order 到 OrderItem 是 one-to-many，若没有明确的预聚合
  策略就必须拒绝；
- `LoyaltyTier`：类型中存在，但模型尚未定义 capability，用于验证缺失能力诊断。

共享的 ontology 方法不知道这些名字，也不知道任何表和字段。`model.telora`
负责把它们映射到物理表达，并最终产生一个无权限的 typed execution plan。
