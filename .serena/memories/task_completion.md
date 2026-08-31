# Task completion
- Run affected package typecheck before final verification and again after changes.
- Run full affected-package test suite plus exact new test files for attributable counts.
- Root required checks: `pnpm run lint` and `pnpm test`; use relevant narrower checks while iterating.
- Frontend production verification: `pnpm --dir frontend build`.
- Re-read every acceptance criterion and implementation note literally.
- Update scoped README/CLAUDE surface if applicable; append dated Agent Notes before setting task status done.
- Stage only task files and commit with `type(scope): summary`; never bypass hooks. Report pre-existing failures or platform gaps explicitly.