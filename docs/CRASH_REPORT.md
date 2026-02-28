-------------------------------------
Translated Report (Full Report Below)
-------------------------------------

Process:               rs-vst-host [23929]
Path:                  /Users/USER/*/rs-vst-host
Identifier:            rs-vst-host
Version:               ???
Code Type:             ARM-64 (Native)
Parent Process:        zsh [3805]
Responsible:           Electron [2819]
User ID:               503

Date/Time:             2026-02-26 05:20:47.4901 +0100
OS Version:            macOS 15.7.4 (24G517)
Report Version:        12
Anonymous UUID:        4E3CA21C-F32E-5C91-EFE4-900A62CBF8B8

Sleep/Wake UUID:       1AEDE988-AB41-4893-8C83-7536F01C9D28

Time Awake Since Boot: 10000 seconds
Time Since Wake:       2688 seconds

System Integrity Protection: enabled

Crashed Thread:        0  main  Dispatch queue: com.apple.main-thread

Exception Type:        EXC_BAD_ACCESS (SIGBUS)
Exception Codes:       KERN_PROTECTION_FAILURE at 0x00000001ffb3d458
Exception Codes:       0x0000000000000002, 0x00000001ffb3d458

Termination Reason:    Namespace SIGNAL, Code 10 Bus error: 10
Terminating Process:   exc handler [23929]

VM Region Info: 0x1ffb3d458 is in 0x1ffb3a028-0x1ffb3d7d8;  bytes after start: 13360  bytes before end: 895
      REGION TYPE                    START - END         [ VSIZE] PRT/MAX SHRMOD  REGION DETAIL
      __AUTH_CONST                1ffb35778-1ffb3a028    [   18K] r--/rw- SM=COW  /usr/lib/libc++.1.dylib
--->  __AUTH_CONST                1ffb3a028-1ffb3d7d8    [   14K] r--/rw- SM=COW  /usr/lib/libc++abi.dylib
      __AUTH_CONST                1ffb3d7d8-1ffb3d8f8    [   288] r--/rw- SM=COW  /usr/lib/system/libsystem_kernel.dylib

Thread 0 Crashed:: main Dispatch queue: com.apple.main-thread
0   ???                           	       0x1ffb3d458 vtable for __cxxabiv1::__class_type_info + 16
1   FabFilter Pro-Q 4             	       0x124643ebc 0x12450c000 + 1277628
2   rs-vst-host                   	       0x1005dd4cc _$LT$rs_vst_host..vst3..instance..Vst3Instance$u20$as$u20$core..ops..drop..Drop$GT$::drop::h1b6e245e7585cfd7 + 180 (instance.rs:695)
3   rs-vst-host                   	       0x1005d6770 core::ptr::drop_in_place$LT$rs_vst_host..vst3..instance..Vst3Instance$GT$::hcad171b71caef6bf + 28 (mod.rs:523)
4   rs-vst-host                   	       0x1005d63b0 core::ptr::drop_in_place$LT$rs_vst_host..audio..engine..AudioEngine$GT$::h67a360e937aa0e05 + 64 (mod.rs:523)
5   rs-vst-host                   	       0x1005d83d8 core::ptr::drop_in_place$LT$core..cell..UnsafeCell$LT$rs_vst_host..audio..engine..AudioEngine$GT$$GT$::h1d91168db97386e9 + 24 (mod.rs:523)
6   rs-vst-host                   	       0x1005d8af0 core::ptr::drop_in_place$LT$std..sync..poison..mutex..Mutex$LT$rs_vst_host..audio..engine..AudioEngine$GT$$GT$::h472ba8c5bb0e5dcd + 72 (mod.rs:523)
7   rs-vst-host                   	       0x10061a4d8 alloc::sync::Arc$LT$T$C$A$GT$::drop_slow::h9c8f991600c4a1a1 + 52 (sync.rs:1910)
8   rs-vst-host                   	       0x1005d9b10 _$LT$alloc..sync..Arc$LT$T$C$A$GT$$u20$as$u20$core..ops..drop..Drop$GT$::drop::hea654623c49f9f84 + 148 (sync.rs:2597)
9   rs-vst-host                   	       0x1005d35cc core::ptr::drop_in_place$LT$alloc..sync..Arc$LT$std..sync..poison..mutex..Mutex$LT$rs_vst_host..audio..engine..AudioEngine$GT$$GT$$GT$::h182e90b0eda39c5e + 24 (mod.rs:523)
10  rs-vst-host                   	       0x1005d5fec core::ptr::drop_in_place$LT$rs_vst_host..gui..backend..ActiveState$GT$::h6905d68d217f7549 + 72 (mod.rs:523)
11  rs-vst-host                   	       0x10060831c rs_vst_host::gui::backend::HostBackend::deactivate_plugin::hd827a3288429abfa + 1088 (backend.rs:367)
12  rs-vst-host                   	       0x1005c18b4 rs_vst_host::gui::app::HostApp::deactivate_active::h3e3e2fe983d1714e + 128 (app.rs:333)
13  rs-vst-host                   	       0x1005c4d98 rs_vst_host::gui::app::HostApp::show_rack::h92ad0610951bff3c + 528 (app.rs:1289)
14  rs-vst-host                   	       0x10059ca7c _$LT$rs_vst_host..gui..app..HostApp$u20$as$u20$eframe..epi..App$GT$::update::_$u7b$$u7b$closure$u7d$$u7d$::ha088895ca0d53923 + 32 (app.rs:593)
15  rs-vst-host                   	       0x1005d20e4 core::ops::function::FnOnce::call_once$u7b$$u7b$vtable.shim$u7d$$u7d$::hb083d9713e302dbe + 36 (function.rs:250)
16  rs-vst-host                   	       0x100cac494 _$LT$alloc..boxed..Box$LT$F$C$A$GT$$u20$as$u20$core..ops..function..FnOnce$LT$Args$GT$$GT$::call_once::hd39fc5f0b8c62a14 + 60
17  rs-vst-host                   	       0x1005fa880 egui::containers::panel::CentralPanel::show_inside_dyn::_$u7b$$u7b$closure$u7d$$u7d$::hc990d3dc6781ffa3 + 128 (panel.rs:1126)
18  rs-vst-host                   	       0x1005d2348 core::ops::function::FnOnce::call_once$u7b$$u7b$vtable.shim$u7d$$u7d$::hdff5bed3800caf4e + 44 (function.rs:250)
19  rs-vst-host                   	       0x100cac494 _$LT$alloc..boxed..Box$LT$F$C$A$GT$$u20$as$u20$core..ops..function..FnOnce$LT$Args$GT$$GT$::call_once::hd39fc5f0b8c62a14 + 60
20  rs-vst-host                   	       0x100c933e0 egui::containers::frame::Frame::show_dyn::h21125748c1bb28f2 + 172
21  rs-vst-host                   	       0x10058df2c egui::containers::frame::Frame::show::ha72e5239adfa72e7 + 180 (frame.rs:416)
22  rs-vst-host                   	       0x1005fa77c egui::containers::panel::CentralPanel::show_inside_dyn::h82ca8ed401a4b392 + 652 (panel.rs:1124)
23  rs-vst-host                   	       0x1005fab4c egui::containers::panel::CentralPanel::show_dyn::h2067227908eb51b5 + 540 (panel.rs:1156)
24  rs-vst-host                   	       0x1005fa924 egui::containers::panel::CentralPanel::show::h88c7564a810a5516 + 124 (panel.rs:1136)
25  rs-vst-host                   	       0x1005c35e0 _$LT$rs_vst_host..gui..app..HostApp$u20$as$u20$eframe..epi..App$GT$::update::ha5b5a2c6e8d78c80 + 712 (app.rs:586)
26  rs-vst-host                   	       0x100827c20 eframe::native::epi_integration::EpiIntegration::update::_$u7b$$u7b$closure$u7d$$u7d$::h3f32abebde0107f3 + 132
27  rs-vst-host                   	       0x1007f78d4 egui::context::Context::run::hba92c3e2f4a12b1b + 368
28  rs-vst-host                   	       0x1008278a8 eframe::native::epi_integration::EpiIntegration::update::hb833530f0f383a30 + 360
29  rs-vst-host                   	       0x10081b374 eframe::native::glow_integration::GlowWinitRunning::run_ui_and_paint::h532d2ff72d104d89 + 2860
30  rs-vst-host                   	       0x100819dfc _$LT$eframe..native..glow_integration..GlowWinitApp$u20$as$u20$eframe..native..winit_integration..WinitApp$GT$::run_ui_and_paint::hb4344323ba00325b + 104
31  rs-vst-host                   	       0x1007fd7bc _$LT$eframe..native..run..WinitAppWrapper$LT$T$GT$$u20$as$u20$winit..application..ApplicationHandler$LT$eframe..native..winit_integration..UserEvent$GT$$GT$::window_event::_$u7b$$u7b$closure$u7d$$u7d$::h5934b42b3e67c8e8 + 100
32  rs-vst-host                   	       0x100838e3c eframe::native::event_loop_context::with_event_loop_context::hd33df197013e06fe + 124
33  rs-vst-host                   	       0x1007fd74c _$LT$eframe..native..run..WinitAppWrapper$LT$T$GT$$u20$as$u20$winit..application..ApplicationHandler$LT$eframe..native..winit_integration..UserEvent$GT$$GT$::window_event::hc812d80347a6e964 + 100
34  rs-vst-host                   	       0x1007deca0 winit::platform::run_on_demand::EventLoopExtRunOnDemand::run_app_on_demand::_$u7b$$u7b$closure$u7d$$u7d$::h19a614708174b290 + 340
35  rs-vst-host                   	       0x1007e5e38 winit::platform_impl::macos::event_loop::map_user_event::_$u7b$$u7b$closure$u7d$$u7d$::h442342f3d13549de + 140
36  rs-vst-host                   	       0x100ae12c4 _$LT$alloc..boxed..Box$LT$F$C$A$GT$$u20$as$u20$core..ops..function..FnMut$LT$Args$GT$$GT$::call_mut::hb808f50533adcd04 + 84
37  rs-vst-host                   	       0x100afec0c winit::platform_impl::macos::event_handler::EventHandler::handle_event::h6378fa45235cb3db + 408
38  rs-vst-host                   	       0x100b20528 winit::platform_impl::macos::app_state::ApplicationDelegate::handle_event::h073a5f38abdcc22a + 208
39  rs-vst-host                   	       0x100b20d40 winit::platform_impl::macos::app_state::ApplicationDelegate::cleared::h1aca56b101844a4f + 756
40  rs-vst-host                   	       0x100aff5b0 winit::platform_impl::macos::observer::control_flow_end_handler::_$u7b$$u7b$closure$u7d$$u7d$::h4f0139512f9db482 + 280
41  rs-vst-host                   	       0x100aff29c winit::platform_impl::macos::observer::control_flow_handler::_$u7b$$u7b$closure$u7d$$u7d$::h52b879381d51950c + 44
42  rs-vst-host                   	       0x100ae32b0 std::panicking::try::do_call::hfc1c2b3aa9a868af + 60
43  rs-vst-host                   	       0x100b2f214 __rust_try + 32
44  rs-vst-host                   	       0x100b255d0 std::panic::catch_unwind::h1e93c170c0d538d3 + 72
45  rs-vst-host                   	       0x100b22c98 winit::platform_impl::macos::event_loop::stop_app_on_panic::h0e7b0ad756e7eee6 + 52
46  rs-vst-host                   	       0x100aff230 winit::platform_impl::macos::observer::control_flow_handler::hf7490824c0031698 + 320
47  rs-vst-host                   	       0x100aff484 winit::platform_impl::macos::observer::control_flow_end_handler::h10bde7fdfc6ae598 + 48
48  CoreFoundation                	       0x190bede28 __CFRUNLOOP_IS_CALLING_OUT_TO_AN_OBSERVER_CALLBACK_FUNCTION__ + 36
49  CoreFoundation                	       0x190bedd10 __CFRunLoopDoObservers + 536
50  CoreFoundation                	       0x190bed420 __CFRunLoopRun + 944
51  CoreFoundation                	       0x190bec9e8 CFRunLoopRunSpecific + 572
52  HIToolbox                     	       0x19c68e27c RunCurrentEventLoopInMode + 324
53  HIToolbox                     	       0x19c6914e8 ReceiveNextEventCommon + 676
54  HIToolbox                     	       0x19c81c484 _BlockUntilNextEventMatchingListInModeWithFilter + 76
55  AppKit                        	       0x194b0da34 _DPSNextEvent + 684
56  AppKit                        	       0x1954ac5cc -[NSApplication(NSEventRouting) _nextEventMatchingEventMask:untilDate:inMode:dequeue:] + 688
57  AppKit                        	       0x194b00be4 -[NSApplication run] + 480
58  rs-vst-host                   	       0x100b56524 _$LT$$LP$$RP$$u20$as$u20$objc2..encode..EncodeArguments$GT$::__invoke::h06fb65ee40019318 + 52
59  rs-vst-host                   	       0x100b56c8c objc2::runtime::message_receiver::msg_send_primitive::send::h7476400eaf6ce7eb + 60
60  rs-vst-host                   	       0x100b4fd28 objc2::runtime::message_receiver::MessageReceiver::send_message::hfba2970537de63e2 + 176
61  rs-vst-host                   	       0x100b3edb4 objc2::__macro_helpers::msg_send::MsgSend::send_message::h0875f8dce37f7c94 + 172
62  rs-vst-host                   	       0x100b3ffec objc2_app_kit::generated::__NSApplication::NSApplication::run::h3501464f63432c94 + 68
63  rs-vst-host                   	       0x1007e63c0 winit::platform_impl::macos::event_loop::EventLoop$LT$T$GT$::run_on_demand::_$u7b$$u7b$closure$u7d$$u7d$::_$u7b$$u7b$closure$u7d$$u7d$::ha1fb3d1fed73cc9c + 168
64  rs-vst-host                   	       0x1007d89b0 objc2::rc::autorelease::autoreleasepool::hb3b27a4ab69b2b9c + 188
65  rs-vst-host                   	       0x1007e62d4 winit::platform_impl::macos::event_loop::EventLoop$LT$T$GT$::run_on_demand::_$u7b$$u7b$closure$u7d$$u7d$::h6da4be4c05d6248d + 44
66  rs-vst-host                   	       0x1007eb2ec winit::platform_impl::macos::event_handler::EventHandler::set::h4fba5420f1f93417 + 564
67  rs-vst-host                   	       0x10080b13c winit::platform_impl::macos::app_state::ApplicationDelegate::set_event_handler::h53c3b837cee4ea6d + 152
68  rs-vst-host                   	       0x1007e626c winit::platform_impl::macos::event_loop::EventLoop$LT$T$GT$::run_on_demand::h835f5cb241a64957 + 256
69  rs-vst-host                   	       0x1007da22c _$LT$winit..event_loop..EventLoop$LT$T$GT$$u20$as$u20$winit..platform..run_on_demand..EventLoopExtRunOnDemand$GT$::run_on_demand::h9628c767c4f493a1 + 120
70  rs-vst-host                   	       0x1007deb40 winit::platform::run_on_demand::EventLoopExtRunOnDemand::run_app_on_demand::hd58de69232414743 + 32
71  rs-vst-host                   	       0x1007fda20 eframe::native::run::run_and_return::hef2fc74239e133d8 + 364
72  rs-vst-host                   	       0x1007fe2ac eframe::native::run::run_glow::_$u7b$$u7b$closure$u7d$$u7d$::h3b0c930b5e15553d + 88
73  rs-vst-host                   	       0x1007fbc10 eframe::native::run::with_event_loop::_$u7b$$u7b$closure$u7d$$u7d$::h563deb327501870f + 356
74  rs-vst-host                   	       0x10080fc98 std::thread::local::LocalKey$LT$T$GT$::try_with::h3d35f1350ea117a7 + 224
75  rs-vst-host                   	       0x10080f734 std::thread::local::LocalKey$LT$T$GT$::with::h37c35982434941aa + 32
76  rs-vst-host                   	       0x1007fba9c eframe::native::run::with_event_loop::h01b58846485b03cb + 96
77  rs-vst-host                   	       0x1007fdff0 eframe::native::run::run_glow::h6326cc9a43b6fb1a + 184
78  rs-vst-host                   	       0x1008280b8 eframe::run_native::h16a0954495b6effb + 532
79  rs-vst-host                   	       0x1005c5060 rs_vst_host::gui::app::launch::h4fa686743bc6f0f1 + 648 (app.rs:1315)
80  rs-vst-host                   	       0x1005e6e78 rs_vst_host::main::h03533197f3040409 + 520 (main.rs:41)
81  rs-vst-host                   	       0x1005d292c core::ops::function::FnOnce::call_once::hbf6aabfe890614bb + 20 (function.rs:250)
82  rs-vst-host                   	       0x1005c7cf4 std::sys::backtrace::__rust_begin_short_backtrace::hf89aadada9eee97f + 24 (backtrace.rs:152)
83  rs-vst-host                   	       0x1005bb6b0 std::rt::lang_start::_$u7b$$u7b$closure$u7d$$u7d$::hd791a032dfa36e55 + 28 (rt.rs:199)
84  rs-vst-host                   	       0x100ef76b4 std::rt::lang_start_internal::h95cf27b851151b9c + 888
85  rs-vst-host                   	       0x1005bb688 std::rt::lang_start::h449106711f9431b8 + 84 (rt.rs:198)
86  rs-vst-host                   	       0x1005e70e0 main + 36
87  dyld                          	       0x190762b98 start + 6076

Thread 1:
0   libsystem_pthread.dylib       	       0x190afeb6c start_wqthread + 0

Thread 2:: caulk.messenger.shared:17
0   libsystem_kernel.dylib        	       0x190ac1bb0 semaphore_wait_trap + 8
1   caulk                         	       0x19c175cc8 caulk::semaphore::timed_wait(double) + 224
2   caulk                         	       0x19c175b70 caulk::concurrent::details::worker_thread::run() + 32
3   caulk                         	       0x19c175844 void* caulk::thread_proxy<std::__1::tuple<caulk::thread::attributes, void (caulk::concurrent::details::worker_thread::*)(), std::__1::tuple<caulk::concurrent::details::worker_thread*>>>(void*) + 96
4   libsystem_pthread.dylib       	       0x190b03bc8 _pthread_start + 136
5   libsystem_pthread.dylib       	       0x190afeb80 thread_start + 8

Thread 3:: caulk.messenger.shared:high
0   libsystem_kernel.dylib        	       0x190ac1bb0 semaphore_wait_trap + 8
1   caulk                         	       0x19c175cc8 caulk::semaphore::timed_wait(double) + 224
2   caulk                         	       0x19c175b70 caulk::concurrent::details::worker_thread::run() + 32
3   caulk                         	       0x19c175844 void* caulk::thread_proxy<std::__1::tuple<caulk::thread::attributes, void (caulk::concurrent::details::worker_thread::*)(), std::__1::tuple<caulk::concurrent::details::worker_thread*>>>(void*) + 96
4   libsystem_pthread.dylib       	       0x190b03bc8 _pthread_start + 136
5   libsystem_pthread.dylib       	       0x190afeb80 thread_start + 8

Thread 4:: caulk::deferred_logger
0   libsystem_kernel.dylib        	       0x190ac1bb0 semaphore_wait_trap + 8
1   caulk                         	       0x19c175cc8 caulk::semaphore::timed_wait(double) + 224
2   caulk                         	       0x19c175b70 caulk::concurrent::details::worker_thread::run() + 32
3   caulk                         	       0x19c175844 void* caulk::thread_proxy<std::__1::tuple<caulk::thread::attributes, void (caulk::concurrent::details::worker_thread::*)(), std::__1::tuple<caulk::concurrent::details::worker_thread*>>>(void*) + 96
4   libsystem_pthread.dylib       	       0x190b03bc8 _pthread_start + 136
...
