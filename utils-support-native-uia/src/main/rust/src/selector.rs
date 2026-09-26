//! JSON 选择器模型：把"怎么找一个控件"从代码里搬到数据里。
//!
//! 之所以做成数据而非硬编码，是因为 IM 类客户端的控件层级会随版本漂移
//! （如微信 3.9 的原生 Qt 控件树与 4.x 的 Electron 树完全不同）。
//! 选择器外置为 JSON 后，调整定位策略只需改配置，不必重编动态库。
//!
//! 所有字符串字段留空表示"不约束该维度"。
//!
//! # 示例
//!
//! ```json
//! {
//!   "controlType": "ListItem",
//!   "nameRegex": "^\\\\(3\\\\)张三$",
//!   "requireEnabled": true,
//!   "children": [
//!     { "controlType": "Text", "name": "10:30" }
//!   ],
//!   "index": 0
//! }
//! ```

use regex::Regex;
use serde::Deserialize;

/// UIA 控件类型名到 `UIA_ControlTypeId` 枚举值的映射表构建。
///
/// # 返回
///
/// 静态映射表（进程内首次调用时构建）。
pub fn control_type_map() -> &'static std::collections::HashMap<&'static str, i32> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static MAP: OnceLock<HashMap<&'static str, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        HashMap::from([
            ("Button", 50000),
            ("Calendar", 50001),
            ("CheckBox", 50002),
            ("ComboBox", 50003),
            ("Edit", 50004),
            ("Hyperlink", 50005),
            ("Image", 50006),
            ("ListItem", 50007),
            ("List", 50008),
            ("Menu", 50009),
            ("MenuBar", 50010),
            ("MenuItem", 50011),
            ("ProgressBar", 50012),
            ("RadioButton", 50013),
            ("ScrollBar", 50014),
            ("Slider", 50015),
            ("Spinner", 50016),
            ("StatusBar", 50017),
            ("Tab", 50018),
            ("TabItem", 50019),
            ("Text", 50020),
            ("ToolBar", 50021),
            ("ToolTip", 50022),
            ("Tree", 50023),
            ("TreeItem", 50024),
            ("Custom", 50025),
            ("Group", 50026),
            ("Thumb", 50027),
            ("DataGrid", 50028),
            ("DataItem", 50029),
            ("Document", 50030),
            ("SplitButton", 50031),
            ("Window", 50032),
            ("Pane", 50033),
            ("Header", 50034),
            ("Footer", 50035),
            ("TitleBar", 50036),
            ("Separator", 50037),
        ])
    })
}

/// 按控件类型名解析 `UIA_ControlTypeId` 枚举值。
///
/// # 参数
///
/// - `name`：控件类型名，如 `ListItem`。`Custom` 在部分 UIA 实现中不被支持，返回 `None`。
///
/// # 返回
///
/// 控件类型枚举值；名称未知时返回 `None`。
pub fn control_type_id(name: &str) -> Option<i32> {
    control_type_map().get(name).copied()
}

/// 选择器：描述如何定位一个 UIA 元素。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Selector {
    /// 控件类型名，如 `ListItem` / `Edit` / `Text`。
    pub control_type: Option<String>,
    /// 控件名称精确匹配。
    pub name: Option<String>,
    /// 控件名称正则匹配（Rust `regex` 语法，不支持反向引用）。
    pub name_regex: Option<String>,
    /// AutomationId 精确匹配。
    pub automation_id: Option<String>,
    /// 窗口类名精确匹配。
    pub class_name: Option<String>,
    /// 宿主进程 ID 精确匹配。
    pub process_id: Option<u32>,
    /// 是否要求控件可用；缺省不过滤。
    pub require_enabled: Option<bool>,
    /// 是否要求控件在屏（未被滚动裁剪）；缺省不过滤。
    pub require_onscreen: Option<bool>,
    /// 相对根元素的遍历最大深度；缺省为不限。
    pub max_depth: Option<u32>,
    /// 必须存在的后代选择器（全部满足才算命中）。
    pub children: Option<Vec<Selector>>,
    /// 向上查找时允许跨越的层级上限，配合 [`Selector::ancestor`] 使用。
    pub child_depth: Option<u32>,
    /// 祖先选择器（满足即命中）。
    pub ancestor: Option<Box<Selector>>,
    /// 命中后取第几个；`None` 表示全部返回，`-1` 表示最后一个。
    pub index: Option<i32>,
}

impl Selector {
    /// 解析选择器 JSON。
    ///
    /// # 参数
    ///
    /// - `json`：选择器 JSON 文本。
    ///
    /// # 返回
    ///
    /// 解析成功返回选择器；失败返回错误描述。
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str::<Selector>(json).map_err(|e| format!("选择器 JSON 解析失败: {e}"))
    }

    /// 判断该选择器是否至少包含一个有效约束维度。
    ///
    /// # 返回
    ///
    /// 全空返回 `false`。
    pub fn has_constraint(&self) -> bool {
        self.control_type.is_some()
            || self.name.is_some()
            || self.name_regex.is_some()
            || self.automation_id.is_some()
            || self.class_name.is_some()
            || self.process_id.is_some()
    }

    /// 校验控件类型名是否受支持。
    ///
    /// # 返回
    ///
    /// 控件类型非法时返回错误描述。
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ct) = &self.control_type {
            if control_type_id(ct).is_none() {
                return Err(format!("不支持的控件类型: {ct}"));
            }
        }
        if let Some(r) = &self.name_regex {
            Regex::new(r).map_err(|e| format!("nameRegex 非法: {e}"))?;
        }
        Ok(())
    }
}

/// 元素属性快照：UIA 查询结果的统一投影。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementInfo {
    /// 元素在上下文池中的句柄（从 1 开始，0 表示无效）。
    pub id: i64,
    /// 控件类型名。
    pub control_type: String,
    /// 控件名称。
    pub name: String,
    /// AutomationId。
    pub automation_id: String,
    /// 窗口类名。
    pub class_name: String,
    /// 宿主进程 ID。
    pub process_id: u32,
    /// 宿主进程名。
    pub process_name: String,
    /// 控件原生窗口句柄（无则为 0）。
    pub native_handle: i64,
    /// 是否可用。
    pub enabled: bool,
    /// 是否在屏。
    pub offscreen: bool,
    /// `Value` 模式的当前值（不支持时为空）。
    pub value: String,
    /// 文本是否只读。
    pub read_only: bool,
    /// 支持的模式名列表。
    pub patterns: Vec<String>,
    /// 屏幕物理坐标矩形。
    pub rect: RectInfo,
}

/// 屏幕物理坐标矩形。
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RectInfo {
    /// 左边界。
    pub left: i32,
    /// 上边界。
    pub top: i32,
    /// 右边界。
    pub right: i32,
    /// 下边界。
    pub bottom: i32,
}

impl RectInfo {
    /// 计算矩形中心点。
    ///
    /// # 返回
    ///
    /// 中心坐标。
    pub fn center(&self) -> (i32, i32) {
        (
            (self.left + self.right) / 2,
            (self.top + self.bottom) / 2,
        )
    }
}
