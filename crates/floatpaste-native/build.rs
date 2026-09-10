fn main() {
    // 单一入口编译：多窗口（速贴 + 悬停预览）经 app.slint 统一导出
    slint_build::compile("ui/app.slint").expect("Slint 编译失败");

    // 嵌入进程清单：声明 Common-Controls v6（muda 静态导入的
    // TaskDialogIndirect 仅存在于 v6）与 PerMonitorV2 DPI
    embed_resource::compile("floatpaste-native.rc", embed_resource::NONE)
        .manifest_required()
        .expect("嵌入进程清单失败");
    println!("cargo:rerun-if-changed=floatpaste-native.rc");
    println!("cargo:rerun-if-changed=floatpaste-native.manifest");
}
