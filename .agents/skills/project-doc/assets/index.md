# Project Doc 索引模板

<!-- 保留适用变体，替换所有占位符，并使用仓库的主语言。 -->

## 根索引变体

~~~markdown
# {{project_name}} 工程文档

> 这是 Project Doc 根索引。加载 project-doc skill 后，在本仓库的第一个任务中完整阅读本文件，再按「阅读时机」进入相关分类。已读内容按 skill 的「阅读与上下文复用」规则复用；内容缺失或需要刷新时再重读。

## 文档范围

{{用一到两句话描述文档库覆盖的持久工程知识，以及不属于文档库的内容。}}

## 根目录文档

- [{{document_title}}]({{document_path}})：{{one_sentence_summary}}。阅读时机：{{read_when}}。

## 分类

- [架构](architecture/index.md)：{{architecture_summary}}。阅读时机：{{architecture_read_when}}。
- [领域](domains/index.md)：{{domains_summary}}。阅读时机：{{domains_read_when}}。
- [接口](interfaces/index.md)：{{interfaces_summary}}。阅读时机：{{interfaces_read_when}}。
- [运维](operations/index.md)：{{operations_summary}}。阅读时机：{{operations_read_when}}。
- [决策](decisions/index.md)：{{decisions_summary}}。阅读时机：{{decisions_read_when}}。

<!-- 搭建期间保留此章节。在文件创建之前将路径显示为代码，创建后再添加链接。 -->
## 搭建状态

- [planned] {{planned_document_path}}: {{planned_scope}}.
- [in_progress] {{current_document_path}}: {{current_scope}}.
~~~

删除没有实际文档的分类。每个保留的分类至少包含一篇文档。搭建完成后，删除「搭建状态」以及所有状态前缀。

## 分类索引变体

~~~markdown
# {{category_name}} 文档索引

> 上级索引：[工程文档](../index.md)

## 范围

{{描述该分类承载的知识，以及路由到其他分类的主题。}}

## 文档列表

- [{{document_title}}]({{document_file}})：{{one_sentence_summary}}。阅读时机：{{read_when}}。

<!-- 搭建期间按需使用 planned、in_progress、verified 或 blocked。搭建完成后移除状态。 -->
~~~

每篇文档在分类索引中只保留一个规范条目。相关分类或文档之间使用普通链接，并将摘要保留在规范条目中。
