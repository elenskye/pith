// 生成文件，请勿手改。源：schemes/pith/tokens/
export const tokens = {
  "scheme": "pith",
  "color": {
    "page": {
      "light": "#f6f5f1",
      "dark": "#1d1c1a"
    },
    "surface": {
      "light": "#fcfbf7",
      "dark": "#252422"
    },
    "raised": {
      "light": "#ffffff",
      "dark": "#2b2a27"
    },
    "tint": {
      "light": "#ebe8e3",
      "dark": "#33312d"
    },
    "line": {
      "light": "#e3e3e0",
      "dark": "#36342f"
    },
    "line-2": {
      "light": "#d3d3d1",
      "dark": "#4a4741"
    },
    "text": {
      "light": "#353534",
      "dark": "#efede8"
    },
    "text-2": {
      "light": "#4a4a46",
      "dark": "#dcd9d2"
    },
    "text-3": {
      "light": "#6b6865",
      "dark": "#c4c0b8"
    },
    "text-4": {
      "light": "#72706d",
      "dark": "#bfbbb3"
    },
    "accent": {
      "light": "#04a5bb",
      "dark": "#2ec4d8"
    },
    "accent-text": {
      "light": "#00758f",
      "dark": "#6fdbe9"
    },
    "on-accent": {
      "light": "#ffffff",
      "dark": "#04191d"
    },
    "second": {
      "light": "#960462",
      "dark": "#ef9fcc"
    },
    "coral": {
      "light": "#f4806b",
      "dark": "#ff9b86"
    },
    "gold": {
      "light": "#ffd09c",
      "dark": "#7a5c33"
    },
    "app-ink": {
      "light": "#2b2a27",
      "dark": "#2b2a27"
    },
    "app-paper": {
      "light": "#fcfbf7",
      "dark": "#fcfbf7"
    },
    "app-coral": {
      "light": "#ff8a72",
      "dark": "#ff8a72"
    }
  },
  "scale": {
    "space": {
      "1": "4px",
      "2": "8px",
      "3": "12px",
      "4": "16px",
      "5": "24px",
      "6": "32px",
      "7": "48px"
    },
    "radius": {
      "s": "6px",
      "m": "8px",
      "l": "14px",
      "pill": "999px"
    },
    "width": {
      "content": "640px",
      "sidenote": "270px",
      "composer": "720px"
    },
    "text": {
      "tiny": "11px",
      "small": "12.5px",
      "ui": "13.5px",
      "body": "17px",
      "lead": "24px",
      "title": "32px",
      "display": "60px"
    },
    "font": {
      "serif": "\"Newsreader\", \"Noto Serif SC\", \"Songti SC\", serif",
      "sans": "\"Lato\", \"Noto Sans SC\", \"PingFang SC\", sans-serif",
      "mono": "ui-monospace, \"SF Mono\", Menlo, monospace"
    }
  },
  "motion": {
    "$note": "动效两档（docs/motion-tiers.md）。calm：控件的小动静，不循环，单次 ≤ 600 毫秒；expressive：只用在下面登记的表面。三种性格是站主 2026-11-23、11-24 定的：舒展（回答出现、点编号看原文）、生长（存进知识库、Logo、顶栏图标）、上墨（悬停）。减弱动态效果时停在最后的样子。",
    "easing": {
      "out": "cubic-bezier(.23,1,.32,1)",
      "spring": "cubic-bezier(.34,1.56,.64,1)",
      "spring-soft": "cubic-bezier(.34,1.36,.64,1)",
      "unfold": "cubic-bezier(.34,1.45,.64,1)",
      "draw": "cubic-bezier(.3,.6,.3,1)",
      "grow": "cubic-bezier(.42,.08,.4,1)",
      "ink": "cubic-bezier(.3,.6,.2,1)",
      "color": "cubic-bezier(.4,0,.2,1)"
    },
    "tiers": {
      "calm": {
        "use": "控件的小动静：悬停上墨、链接下划线、行上的珊瑚点、卡片抬起、页边注展开。",
        "stateMs": [
          160,
          320
        ],
        "enterMs": 520,
        "enterEasing": "ink",
        "maxMsWithoutSignature": 600,
        "forbid": [
          "loop",
          "parallax",
          "page-transition"
        ]
      },
      "expressive": {
        "use": "招牌时刻和 Agent 在做事的图标，要登记。",
        "maxSingleMs": 2400,
        "registeredSurfacesOnly": true,
        "loopRules": "只有「Pith 在想」的 Agent 图标能循环（等的时候）；减弱动态效果时停在一帧看得懂的样子",
        "registeredSurfaces": [
          "logo-grow",
          "topbar-grow",
          "agent-status",
          "answer-unfold",
          "save-grow",
          "palette-lake"
        ]
      }
    },
    "personalities": {
      "unfold": {
        "label": "舒展",
        "use": "回答一段段出现、点编号时页边注展开",
        "foldMs": 900,
        "staggerMs": 400,
        "easing": "unfold",
        "sidenoteMs": 550
      },
      "grow": {
        "label": "生长",
        "use": "存进知识库（种子沿弧线落到图标正中）、Logo、顶栏图标",
        "easing": "grow",
        "seedMs": 820,
        "fedMs": 700,
        "coralLast": true
      },
      "ink": {
        "label": "上墨",
        "use": "悬停：纸片滑进来、细线重写、实心压一下、珊瑚点最后抵达（借 Vermilion 的做法，数值是 Pith 的）",
        "paperMs": 200,
        "writeMs": 300,
        "pressMs": 320,
        "arriveMs": 520,
        "linkMs": 220
      }
    },
    "colorTransition": {
      "ms": 900,
      "easing": "color",
      "use": "换配色时颜色 token 慢慢过渡（@property 注册成 <color>）"
    },
    "registeredSurfaces": {
      "logo-grow": {
        "tier": "expressive",
        "label": "Logo 从底下长出来，结一颗果",
        "personality": "grow",
        "trigger": "App 打开时一次；鼠标放到 Logo 上",
        "maxMs": 1740,
        "loop": false
      },
      "topbar-grow": {
        "tier": "expressive",
        "label": "顶栏三个图标悬停时各自长出来：知识库从底下长、书一本本摆好、轮生的叶转出来",
        "personality": "grow",
        "trigger": "鼠标放到顶栏的知识库、文献库、设置上",
        "maxMs": 1320,
        "loop": false
      },
      "agent-status": {
        "tier": "expressive",
        "label": "Pith 在想：状态行上 Agent 正在做的那一步的图标（14 个，一圈 1.2–2.6 秒）",
        "trigger": "Agent 调一个工具时换成那个图标，做完停住",
        "loop": true,
        "loopMs": 2600
      },
      "answer-unfold": {
        "tier": "expressive",
        "label": "回答一段段舒展开",
        "personality": "unfold",
        "trigger": "回答写出来",
        "maxMs": 1400,
        "loop": false
      },
      "save-grow": {
        "tier": "expressive",
        "label": "存进知识库：种子落到顶栏的知识库图标，图标晃一下、芽弹一下、+1",
        "personality": "grow",
        "trigger": "点交付卡的「存进……」",
        "maxMs": 1900,
        "loop": false
      },
      "palette-lake": {
        "tier": "expressive",
        "label": "换配色：五圈湖面波纹加一层水面的扭曲",
        "trigger": "设置 › 外观、⌘K 换配色",
        "maxMs": 2300,
        "loop": false
      }
    }
  },
  "defaultPalette": "paper-sea",
  "palettes": {
    "paper-sea": {
      "label": "纸与海",
      "family": "Paper & Sea",
      "color": {
        "page": {
          "light": "#f6f5f1",
          "dark": "#1d1c1a"
        },
        "surface": {
          "light": "#fcfbf7",
          "dark": "#252422"
        },
        "raised": {
          "light": "#ffffff",
          "dark": "#2b2a27"
        },
        "tint": {
          "light": "#ebe8e3",
          "dark": "#33312d"
        },
        "line": {
          "light": "#e3e3e0",
          "dark": "#36342f"
        },
        "line-2": {
          "light": "#d3d3d1",
          "dark": "#4a4741"
        },
        "text": {
          "light": "#353534",
          "dark": "#efede8"
        },
        "text-2": {
          "light": "#4a4a46",
          "dark": "#dcd9d2"
        },
        "text-3": {
          "light": "#6b6865",
          "dark": "#c4c0b8"
        },
        "text-4": {
          "light": "#72706d",
          "dark": "#bfbbb3"
        },
        "accent": {
          "light": "#04a5bb",
          "dark": "#2ec4d8"
        },
        "accent-text": {
          "light": "#00758f",
          "dark": "#6fdbe9"
        },
        "on-accent": {
          "light": "#ffffff",
          "dark": "#04191d"
        },
        "second": {
          "light": "#960462",
          "dark": "#ef9fcc"
        },
        "coral": {
          "light": "#f4806b",
          "dark": "#ff9b86"
        },
        "gold": {
          "light": "#ffd09c",
          "dark": "#7a5c33"
        },
        "app-ink": {
          "light": "#2b2a27",
          "dark": "#2b2a27"
        },
        "app-paper": {
          "light": "#fcfbf7",
          "dark": "#fcfbf7"
        },
        "app-coral": {
          "light": "#ff8a72",
          "dark": "#ff8a72"
        }
      }
    },
    "mist-ochre": {
      "label": "雾与赭",
      "family": "Mist & Ochre",
      "color": {
        "page": {
          "light": "#eef0f2",
          "dark": "#191c21"
        },
        "surface": {
          "light": "#f7f8fa",
          "dark": "#20242a"
        },
        "raised": {
          "light": "#ffffff",
          "dark": "#262b32"
        },
        "tint": {
          "light": "#e2e6eb",
          "dark": "#2a2f36"
        },
        "line": {
          "light": "#dde1e6",
          "dark": "#2d3239"
        },
        "line-2": {
          "light": "#c8cdd4",
          "dark": "#414851"
        },
        "text": {
          "light": "#22272e",
          "dark": "#e9ecf0"
        },
        "text-2": {
          "light": "#3a414b",
          "dark": "#d2d7de"
        },
        "text-3": {
          "light": "#58606b",
          "dark": "#b6bdc7"
        },
        "text-4": {
          "light": "#656e77",
          "dark": "#b4bbc5"
        },
        "accent": {
          "light": "#c26a26",
          "dark": "#e8a265"
        },
        "accent-text": {
          "light": "#9a4f14",
          "dark": "#f0b57e"
        },
        "on-accent": {
          "light": "#ffffff",
          "dark": "#1a1006"
        },
        "second": {
          "light": "#34558c",
          "dark": "#a3bce8"
        },
        "coral": {
          "light": "#de6a55",
          "dark": "#f59a85"
        },
        "gold": {
          "light": "#f0d3a4",
          "dark": "#6a5230"
        },
        "app-ink": {
          "light": "#22272e",
          "dark": "#22272e"
        },
        "app-paper": {
          "light": "#f7f8fa",
          "dark": "#f7f8fa"
        },
        "app-coral": {
          "light": "#f08a74",
          "dark": "#f08a74"
        }
      }
    }
  }
} as const;
export type ColorRole = keyof typeof tokens.color;
