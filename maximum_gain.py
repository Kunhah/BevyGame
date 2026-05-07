import numpy as np
import matplotlib.pyplot as plt

def growth_tactical(attr, base, exponent, base_divisor=8):
    """Tactical RPG growth with smaller numbers"""
    shifted_attr = 2 * attr
    power = shifted_attr ** exponent
    divided_power = power / 524288
    value = (base / base_divisor) - divided_power
    return max(0, value)

def analyze_curve(base, exponent, max_attr, base_divisor=8):
    """Analyze total growth and distribution"""
    total = 0
    growths = []
    
    print(f"\n{'='*50}")
    print(f"BASE: {base} (each level gives up to {base/base_divisor:.1f})")
    print(f"EXPONENT: {exponent}")
    print(f"MAX ATTR: {max_attr}")
    print(f"{'='*50}")
    
    for attr in range(max_attr + 1):
        growth = growth_tactical(attr, base, exponent, base_divisor)
        growths.append(growth)
        total += growth
        
        # Show milestone values
        if attr in [0, max_attr//4, max_attr//2, 3*max_attr//4, max_attr]:
            print(f"Attr {attr:3d}: growth = {growth:5.2f}")
    
    print(f"\nTOTAL GROWTH: {total:.1f}")
    print(f"AVERAGE/LEVEL: {total/max_attr:.2f}")
    
    # Growth distribution by quartiles
    quartiles = [(0, max_attr//4), (max_attr//4, max_attr//2), 
                 (max_attr//2, 3*max_attr//4), (3*max_attr//4, max_attr)]
    
    print(f"\nGROWTH DISTRIBUTION:")
    for i, (start, end) in enumerate(quartiles):
        q_total = sum(growths[start:end+1])
        q_pct = (q_total / total) * 100
        print(f"  Q{i+1} (Lv{start}-{end}): {q_total:.1f} ({q_pct:.1f}%)")
    
    return growths, total

# Test different configurations
configs = [
    {"name": "Custom test", "base": 100, "exponent": 4.243755665, "max_attr": 15},
]

print("TACTICAL RPG GROWTH CALCULATOR")
print("="*50)

results = {}
for config in configs:
    print(f"\n{config['name']}")
    growths, total = analyze_curve(config["base"], config["exponent"], 
                                   config["max_attr"], base_divisor=8)
    results[config["name"]] = {"growths": growths, "total": total}

# Find exponent for specific target (when growth reaches 0 at max_attr)
# def find_exponent_for_target(base, max_attr, base_divisor=8):
#     """Calculate exponent needed so growth reaches 0 at max_attr"""
#     target_growth = base / base_divisor  # Target value we want to reach zero
#     target_power = target_growth * 524288  # Reverse the formula
#     shifted_max = 2 * max_attr
#     exponent = np.log(target_power) / np.log(shifted_max)
#     return exponent

# print("\n" + "="*50)
# print("EXPONENT CALCULATOR (for growth to reach 0 at max level)")
# print("="*50)

# for base in [30, 50, 80, 100]:
#     for max_attr in [30, 50, 70]:
#         exponent = find_exponent_for_target(base, max_attr)
#         total_expected = analyze_curve(base, exponent, max_attr, base_divisor=8)[1]
#         print(f"Base={base:3d}, MaxAttr={max_attr:2d}: Exponent={exponent:.4f} → Total Growth={total_expected:.1f}")

# Plot comparison
# fig, axes = plt.subplots(1, 3, figsize=(15, 5))
# for idx, (name, data) in enumerate(results.items()):
#     ax = axes[idx]
#     growths = data["growths"]
#     ax.plot(range(len(growths)), growths, 'b-', linewidth=2)
#     ax.fill_between(range(len(growths)), growths, alpha=0.3)
#     ax.set_title(f"{name}\nTotal: {data['total']:.1f}")
#     ax.set_xlabel("Attribute Level")
#     ax.set_ylabel("Growth per Level")
#     ax.grid(True, alpha=0.3)
#     ax.set_ylim(bottom=0)

# plt.tight_layout()
# plt.show()

# Recommended balanced configs
# print("\n" + "="*50)
# print("RECOMMENDED BALANCED CONFIGS FOR TACTICAL RPG")
# print("="*50)

# recommendations = [
#     {"stat": "HP", "base": 80, "max_level": 50, "exponent": 2.95, "total_growth": "~200"},
#     {"stat": "MP/Energy", "base": 40, "max_level": 40, "exponent": 2.85, "total_growth": "~100"},
#     {"stat": "Attack", "base": 60, "max_level": 50, "exponent": 3.05, "total_growth": "~150"},
#     {"stat": "Defense", "base": 60, "max_level": 50, "exponent": 3.05, "total_growth": "~150"},
#     {"stat": "Speed", "base": 50, "max_level": 50, "exponent": 3.00, "total_growth": "~125"},
# ]

# for rec in recommendations:
#     print(f"\n{rec['stat']}:")
#     print(f"  Base: {rec['base']} (max growth {rec['base']/8:.1f} per level)")
#     print(f"  Max Level: {rec['max_level']}")
#     print(f"  Exponent: {rec['exponent']}")
#     print(f"  Expected Total Growth: {rec['total_growth']}")
