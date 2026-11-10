//! 服务端打印渲染：HTML → PDF（本机 Chrome CDP）。
//! 实现本体已下沉 `rust_utils::printrender`（浏览器探测/no-sandbox 判定/
//! 进程级复用/CDP 泵随库走）；本模块保留服务面别名与 [`Renderer`] 导出，
//! 消费者路径不变。

pub use rust_utils::printrender::{render_pdf, PrintOptions};
// 类型面完整性：Renderer trait 与默认实现随库走，供部署侧换伴生容器实现。
#[allow(unused_imports)]
pub use rust_utils::printrender::{ChromeRenderer, Renderer};

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_resolve() {
        // 类型面存在性：trait 与默认实现都在
        fn assert_renderer<T: rust_utils::printrender::Renderer>() {}
        assert_renderer::<rust_utils::printrender::ChromeRenderer>();
    }
}
