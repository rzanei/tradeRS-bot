## 💰 Recommended Capital Allocation for DCA Auto-Trading Bot

This table outlines suggested capital usage, DCA scaling, and risk exposure across different portfolio sizes when using this DCA trading strategy with Jupiter on Solana.

| Total Capital | Max Entry Size | DCA Level 1 | DCA Level 2 | Estimated Drawdown Risk | Suitability         |
|---------------|----------------|-------------|-------------|--------------------------|----------------------|
| $100          | $25            | $12.50      | $6.25       | **High** – ~60%          | ❌ Not recommended   |
| $300          | $75            | $37.50      | $18.75      | ~40%                     | ⚠️ For testing only  |
| $500          | $100           | $50.00      | $25.00      | ~35%                     | ✅ Entry-level viable |
| $1,000        | $200           | $100.00     | $50.00      | ~25–30%                  | ✅ Stable             |
| $2,000        | $300           | $150.00     | $75.00      | ~20%                     | ✅ Recommended        |
| $3,000        | $400           | $200.00     | $100.00     | ~15%                     | ✅ Scalable           |
| $5,000        | $500           | $250.00     | $125.00     | ~10–12%                  | ✅ Solid strategy     |
| $7,500        | $600           | $300.00     | $150.00     | <10%                     | ✅ Safe for compounding |
| $10,000       | $750           | $375.00     | $187.50     | <8%                      | ✅ Ideal scaling      |

> 💡 **DCA Level 1 and Level 2** represent recovery steps on price dips. Risk represents the max capital exposed if DCA is fully triggered before recovery.



service path:
/etc/systemd/system/tradeRS_Bot_Controller.service