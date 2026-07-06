# 团队开发与贡献指南 (CONTRIBUTING)

欢迎参与实验室安全管理系统 (Lab Safety System) 的开发！为了保证团队协作的高效性、代码质量以及项目工程的标准化，请大家在开发过程中共同遵守以下规范。

---

## 1. 分支管理策略

本项目采用经典的 **Git Flow / Feature Branch** 工作流：

- **`main` 分支**: 生产/主稳定分支。任何时候都不允许直接向 `main` 分支提交代码。所有变更必须通过 Pull Request (PR) 并经过 CI 及 Code Review 后才能并入。
- **功能分支 (`feature/*`)**: 用于新功能的开发。从最新的 `main` 检出，命名格式推荐为 `feature/issue-photo-upload` 或 `feature/issue-123`。
- **修复分支 (`bugfix/*` 或 `hotfix/*`)**: 用于修复缺陷。从 `main` 检出，命名格式推荐为 `bugfix/login-error` 或 `hotfix/session-leak`。

---

## 2. 代码提交规范 (Commit Message Rules)

为了保持提交历史的清晰易读，我们强制推行 **Conventional Commits (约定式提交)** 规范。提交信息的格式如下：

```text
<type>(<scope>): <subject>

[optional body]
```

### 2.1 常用类型 (`type`)

- **`feat`**: 引入新功能。
- **`fix`**: 修复 Bug。
- **`docs`**: 仅修改文档 (如 README, CONTRIBUTING, 协作指南等)。
- **`style`**: 仅修改代码格式 (空格、分号、格式化等，不影响代码逻辑)。
- **`refactor`**: 代码重构 (既不是新增功能，也不是修复 Bug)。
- **`test`**: 新增或修改测试用例。
- **`chore`**: 构建工具、依赖库更新或辅助工具的变动。

### 2.2 示例

```text
feat(hazard): 支持隐患整改照片的多文件上传功能
fix(auth): 修复 SSO 回调在缺少 redirect 参数时的空指针异常
docs(readme): 在快速部署章节补充 https 部署指南
```

---

## 3. 代码质量要求与本地自测

每次提交代码或发起 PR 之前，请务必在本地运行以下校验，确保 CI 管道能够顺利通过。

### 3.1 后端 Rust 代码要求
后端需严格保证零 Formatting 错误和零 Clippy 警告：
- **格式化检查**:
  ```bash
  cargo fmt --all -- --check
  ```
- **静态代码分析**:
  ```bash
  cargo clippy --all-targets -- -D warnings
  ```
- **单元与集成测试**:
  ```bash
  cargo test --all-targets
  ```

### 3.2 前端 React / TS 代码要求
- **构建与类型检查**:
  ```bash
  cd frontend
  npm run build
  ```
- **提交规范**: 请避免在代码中残留无用的调试语句（如 `console.log`）或未使用的变量。

### 3.3 开启本地自动把关 (Git Pre-commit Hook)

为了避免不合规的代码被意外 commit 进仓库，项目中提供了一个轻量高效的 pre-commit 脚本。它会在您执行 `git commit` 时自动触发，校验 Rust 格式、Clippy 及前端编译。

请在项目根目录下执行以下命令将其激活：
```bash
ln -sf ../../scripts/pre-commit .git/hooks/pre-commit
```
激活后，您的每次 `git commit` 都会在本地自动进行前置把关，保障提交质量。


---

## 4. Pull Request 规范

1. **提交前同步**: 发起 PR 前，请确保已将 `main` 的最新代码合并（或 rebase）到你的功能分支中。
2. **描述清晰**: PR 的描述中应清晰阐述：
   - 解决了什么问题，或实现了什么功能。
   - 是否包含破坏性变更 (Breaking Changes)。
   - 关联的 Issue 编号（如果有）。
3. **通过 CI**: 只有在 GitHub Actions CI 检查（格式、Clippy、编译、测试等）全部通过，且至少有一名其他团队成员 Approve 后，方可并入 `main`。
