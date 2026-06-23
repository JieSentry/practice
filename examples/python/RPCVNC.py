# -*- coding: utf-8 -*-
import pywinauto
import grpc
import ctypes
import os
import time
import threading
import random
import datetime
from dotenv import load_dotenv
from vncdotool import api as vnc_api
from concurrent.futures import ThreadPoolExecutor
from pywinauto import WindowSpecification
from pywinauto.application import Application

# 加载VNC和坐标配置
load_dotenv()
VNC_HOST = os.getenv("VNC_HOST", "127.0.0.1")
VNC_PORT = int(os.getenv("VNC_PORT", 5900))
VNC_PASSWORD = os.getenv("VNC_PASSWORD", "")

# 非游戏UI区域裁剪参数 - 可通过PowerToys Screen Ruler测量后在.env中配置
CROP_LEFT_PX = int(os.getenv("CROP_LEFT_PX", 4))    # 左侧非游戏区域宽度
CROP_TOP_PX = int(os.getenv("CROP_TOP_PX", 78))     # 顶部非游戏区域高度（VM工具栏）
GAME_WIDTH = int(os.getenv("GAME_WIDTH", 1366))     # 游戏实际显示宽度
GAME_HEIGHT = int(os.getenv("GAME_HEIGHT", 768))    # 游戏实际显示高度

# 系统API（仅用于本地窗口匹配，不影响远程VNC）
user32 = ctypes.windll.user32

def get_utc8_timestamp():
    """生成毫秒级时间戳，格式：YYYY-MM-DD HH:MM:SS.xxx"""
    return datetime.datetime.now().strftime("%m-%d %H:%M")

# gRPC proto相关导入
from input_pb2 import (
    Key,
    KeyRequest,
    KeyResponse,
    KeyDownRequest,
    KeyDownResponse,
    KeyUpRequest,
    KeyUpResponse,
    KeyInitRequest,
    KeyInitResponse,
    MouseRequest,
    MouseResponse,
    MouseAction,
    Coordinate,
    KeyState,
    KeyStateRequest,
    KeyStateResponse,
)
from input_pb2_grpc import KeyInputServicer, add_KeyInputServicer_to_server


class VNCInputClient:
    """VNC输入客户端，统一使用keyPress方法处理所有按键操作"""
    def __init__(self, host: str, port: int, password: str = ""):
        self.host = host
        self.port = port
        self.password = password
        self.client = None
        self.vnc_lock = threading.RLock()  # 可重入锁，避免死锁
        self.retry_times = 3               # 操作重试次数
        self.connect()

    def _get_random_delay(self):
        """生成-0.01~0.01s的按键延迟（结合多区间+正态分布+噪声+动态精度）"""
        # 1. 多区间加权选择：模拟人类操作的波动特征（核心小波动占70%，中等20%，边缘10%）
        delay_ranges = [
            (-0.003, 0.003),   # 核心区间（70%概率）：小波动，最贴近人类常态
            (-0.007, -0.004),  # 中等负区间（10%概率）：稍大的负波动
            (0.004, 0.007),    # 中等正区间（10%概率）：稍大的正波动
            (-0.01, -0.008),   # 边缘负区间（5%概率）：最大的负波动
            (0.008, 0.01)      # 边缘正区间（5%概率）：最大的正波动
        ]
        # 合并weights到choices调用，简化代码（也可保留单独weights定义，二选一）
        chosen_range = random.choices(delay_ranges, weights=[0.7, 0.1, 0.1, 0.05, 0.05])[0]
        range_min, range_max = chosen_range
        # 2. 区间内用正态分布生成核心延迟（均值=区间中点，标准差=区间宽度/6，确保6σ覆盖区间）
        range_mid = (range_min + range_max) / 2  # 区间中点（正态分布均值）
        range_std = (range_max - range_min) / 6  # 标准差，6σ覆盖整个区间
        core_delay = random.gauss(range_mid, range_std)
        # 3. 兜底限制核心延迟在选定区间内（避免正态分布偶尔超出区间）
        core_delay = max(range_min, min(range_max, core_delay))
        # 4. 叠加随机噪声（1e-10级别）：让小数位完全无规律，避免机械重复
        noise = random.random() * 1e-10  # 0~1e-10的极小随机数
        final_delay = core_delay + noise
        # 5. 最终兜底：确保整体范围严格在-0.01~0.01之间（双重保障）
        final_delay = max(-0.01, min(0.01, final_delay))
        # 6. 动态精度：随机保留10~18位小数（替代固定15位，彻底避免机械感）
        random_prec = random.randint(10, 18)
        final_delay = round(final_delay, random_prec)
        return final_delay

    def _click_delay(self):
        """生成0.08~0.17s的点击延迟（结合多区间+正态分布+噪声+动态精度）"""
        # 1. 模拟人类不同反应速度：快/中/慢 加权随机选区间
        delay_ranges = [
            (0.08, 0.10),  # 快反应（20%概率）
            (0.11, 0.15),  # 中等反应（70%概率，最贴近人类常态）
            (0.16, 0.17)   # 慢反应（10%概率）
        ]
        chosen_range = random.choices(delay_ranges, weights=[0.2, 0.7, 0.1])[0]
        range_min, range_max = chosen_range
        # 2. 区间内用正态分布生成核心延迟（均值取区间中点，标准差缩小）
        range_mid = (range_min + range_max) / 2
        range_std = (range_max - range_min) / 6  # 6σ覆盖整个区间
        core_delay = random.gauss(range_mid, range_std)
        # 3. 限制核心延迟在选定区间内
        core_delay = max(range_min, min(range_max, core_delay))
        # 4. 叠加随机噪声（10^-10级别，让小数位完全无规律）
        noise = random.random() * 1e-10
        final_delay = core_delay + noise
        # 5. 动态精度：随机保留10~18位小数（无固定/通常位数）
        random_prec = random.randint(10, 18)
        return round(final_delay, random_prec)

    def connect(self):
        """建立VNC连接，增加连接重试"""
        with self.vnc_lock:
            for retry in range(self.retry_times):
                try:
                    self.client = vnc_api.connect(
                        f"{self.host}::{self.port}",
                        password=self.password,
                        timeout=10
                    )
                    print(f"✅[{get_utc8_timestamp()}]成功连接到VNC服务器 {self.host}:{self.port}")
                    return
                except Exception as e:
                    if retry == self.retry_times - 1:
                        raise ConnectionError(f"VNC连接失败（重试{self.retry_times}次）: {str(e)}")
                    print(f"⚠️  VNC连接重试 {retry+1}/{self.retry_times}，错误: {e}")
                    time.sleep(0.5)

    def reconnect(self):
        """重连VNC（连接断开时调用）"""
        with self.vnc_lock:
            self.close()
            time.sleep(1)  # 重连前等待1秒，避免频繁重试
            self.connect()

    def close(self):
        """关闭VNC连接"""
        with self.vnc_lock:
            if self.client:
                try:
                    self.client.close()
                except Exception as e:
                    print(f"⚠️  关闭VNC连接时出现异常: {str(e)}")
                self.client = None

    def send_mouse_action(self, x: int, y: int, action: MouseAction):
        """VNC鼠标操作，增强重试和异常隔离"""
        if not self.client:
            self.reconnect()
        # 鼠标操作增加重试，避免单次失败导致中断
        for retry in range(self.retry_times):
            try:
                with self.vnc_lock:  # 仅单次VNC操作加锁
                    self.client.mouseMove(x, y)
                    if action == MouseAction.Move:
                        print(f"✅[{get_utc8_timestamp()}]执行鼠标移动成功，坐标: x={x}, y={y}")
                    elif action == MouseAction.Click:
                        click_delay = self._click_delay()
                        delay_str = str(click_delay)
                        decimal_len = len(delay_str.split('.')[-1]) if '.' in delay_str else 0
                        # 核心修复：使用vncdotool标准的mouseDown/mouseUp
                        self.client.mouseDown(1)
                        time.sleep(click_delay)
                        self.client.mouseUp(1)
                        time.sleep(0.1)
                        print(f"✅[{get_utc8_timestamp()}]执行鼠标左键点击成功，坐标: x={x}, y={y}，点击时间{click_delay}，小数位数量：{decimal_len}")
                    elif action == MouseAction.ScrollDown:
                        self.client.scroll(-1)
                        print(f"✅[{get_utc8_timestamp()}]执行鼠标滚轮向下成功")
                return  # 操作成功则退出重试
            except Exception as e:
                err_msg = f"VNC鼠标操作失败（重试{retry+1}/{self.retry_times}）: {str(e)}"
                print(f"❌ {err_msg}")
                if retry < self.retry_times - 1:
                    self.reconnect()
                    time.sleep(0.2)
                else:
                    print(f"❌[{get_utc8_timestamp()}]鼠标操作最终失败，坐标: x={x}, y={y}, 动作={action}")

    def send_key_press(self, key: str, hold_time: float = 0.05):
        """
        统一按键处理方法：按下→保持→释放（原子性保证）
        所有按键操作都通过此方法处理，包括单独的按下/释放请求
        """
        if not self.client:
            self.reconnect()
            
        vnc_key = self._map_key_to_vnc_format(key)
        pressed = False
        
        try:
            # 1. 按下按键（仅单次IO加锁，完成立即释放锁）
            for retry in range(self.retry_times):
                try:
                    with self.vnc_lock:  # 仅keyDown操作加锁
                        self.client.keyDown(vnc_key)
                    pressed = True
                    break
                except Exception as e:
                    print(f"❌ 按键按下失败（重试{retry+1}/{self.retry_times}）: {str(e)}")
                    self.reconnect()
                    time.sleep(0.2)
            
            # 2. 保持按键（锁已释放，其他按键可在此期间执行按下/释放）
            if pressed and hold_time > 0:
                # 仅在按下时长添加随机延迟，按下前/释放无延迟
                total_hold_time = hold_time + self._get_random_delay()
                # 确保延迟不为负数
                total_hold_time = max(0.001, total_hold_time)
                time.sleep(total_hold_time)
                print(f"   ⏳ 按键保持时间: 基础{hold_time*1000:.1f}ms + 随机{self._get_random_delay()*1000:.10f}ms = 总计{total_hold*1000:.10f}ms")
                
                # 3. 释放按键（仅单次IO加锁）
                for retry in range(self.retry_times):
                    try:
                        with self.vnc_lock:  # 仅keyUp操作加锁
                            self.client.keyUp(vnc_key)
                        return True
                    except Exception as e:
                        print(f"❌ 按键释放失败（重试{retry+1}/{self.retry_times}）: {str(e)}")
                        self.reconnect()
                        time.sleep(0.2)
            elif pressed and hold_time == 0:
                # 单独的按下请求，只按不松（保持键位无延迟）
                return True
                
        except Exception as e:
            print(f"❌ 按键操作异常: {str(e)}")
            # 兜底：如果按下了但没释放，尝试释放
            if pressed and hold_time > 0:
                try:
                    with self.vnc_lock:
                        self.client.keyUp(vnc_key)
                    print(f"   ⚠️  异常兜底：强制释放按键 {vnc_key}")
                except:
                    pass
        return False

    def send_key_release(self, key: str):
        """
        单独释放按键（内部仍基于keyPress逻辑，快速释放）
        """
        if not self.client:
            self.reconnect()
            
        vnc_key = self._map_key_to_vnc_format(key)
        try:
            for retry in range(self.retry_times):
                try:
                    with self.vnc_lock:  # 仅单次IO加锁
                        self.client.keyUp(vnc_key)
                    return True
                except Exception as e:
                    print(f"❌ 按键释放失败（重试{retry+1}/{self.retry_times}）: {str(e)}")
                    self.reconnect()
                    time.sleep(0.2)
        except Exception as e:
            print(f"❌ 释放按键异常: {str(e)}")
        return False

    def _map_key_to_vnc_format(self, key: str) -> str:
        """完善VNC按键映射"""
        key_mapping = {
            # 修饰键（内置别名+编码双兼容）
            "ctrl": "ctrl",
            "shift": "shift",
            "alt": "alt",
            # 方向键（内置别名）
            "UP": "up",
            "DOWN": "down",
            "LEFT": "left",
            "RIGHT": "right",
            "HOME": "home",   
            "END": "end",        
            "PGUP": "pgup",
            "PGDN": "pgdn",
            "INSERT": "ins",
            "DEL": "del",
            "ESC": "esc",
            "ENTER": "return",    
            "SPACE": "space",   
            "Backspace": "bsp",   
            "F1": "f1", "F2": "f2", "F3": "f3", "F4": "f4",
            "F5": "f5", "F6": "f6", "F7": "f7", "F8": "f8",
            "F9": "f9", "F10": "f10", "F11": "f11", "F12": "f12",
            "\\": "bslash"
        }
        return key_mapping.get(key, key.lower())


class KeyInput(KeyInputServicer):
    def __init__(
        self,
        keys_map: dict[Key, str],
        vnc_client: VNCInputClient
    ) -> None:
        super().__init__()
        self.keys_map = keys_map
        self.vnc_client = vnc_client
        self.key_state_cache = {key: KeyState.Released for key in Key.values()}
        self.request_counter = 0  # 跟踪请求数，排查指令中断问题
        
        # 新增：组合键粘连配置
        self.combo_stick_time = 0.15  # 组合键粘连时长（150ms，可根据需求调整）
        # 替换：线程池替代单个线程引用，限制最大并发数（建议5-10）
        self.release_executor = ThreadPoolExecutor(max_workers=5, thread_name_prefix="key_release_")
        self.release_futures = []  # 跟踪未完成的释放任务，用于定期清理
        self.key_state_update_time = {key: time.time() for key in Key.values()}  # 记录键位更新时间（用于缓存清理）
        
        # 启动定时清理线程（守护线程，随主线程退出）
        self.clean_thread = threading.Thread(target=self._clean_loop, daemon=True)
        self.clean_thread.start()

    def Init(self, request: KeyInitRequest, context):
        """BOT初始化接口，更新坐标配置说明"""
        print(f"\n📌[{get_utc8_timestamp()}]收到BOT初始化请求 Init，seed: {request.seed.hex()}")
        return KeyInitResponse(mouse_coordinate=Coordinate.Relative)

    def KeyState(self, request: KeyStateRequest, context):
        """返回VNC远程按键状态"""
        key = request.key
        key_name = self.keys_map.get(key, f"未知按键({key})")
        state = self.key_state_cache.get(key, KeyState.Released)
        return KeyStateResponse(state=state)

    def SendMouse(self, request: MouseRequest, context):
        """
        鼠标操作处理逻辑（修复：正确扣除顶部78px工具栏）
        1. 接收机器人传入的Relative坐标
        2. 裁剪左侧+顶部非游戏区域
        3. 转换为VNC绝对坐标并执行操作
        """
        self.request_counter += 1
        # 机器人传入的原始参数
        input_width = request.width    
        input_height = request.height  
        input_x = request.x            
        input_y = request.y            
        action = request.action
        try:
            # 核心修复：正确扣除顶部78px工具栏的坐标转换逻辑
            # 1. 减去左侧+顶部裁剪偏移
            adjusted_x = input_x - CROP_LEFT_PX
            adjusted_y = input_y - CROP_TOP_PX  # 关键修复：扣除顶部78px工具栏
            # 2. 计算有效游戏区域（扣除左侧+顶部非游戏区域）
            valid_width = input_width - CROP_LEFT_PX
            valid_height = input_height - CROP_TOP_PX  # 关键修复：有效高度扣除顶部78px
            # 3. 相对坐标转VNC绝对坐标（按游戏分辨率比例转换）
            vnc_x = int((adjusted_x / valid_width) * GAME_WIDTH) if valid_width > 0 else 0
            vnc_y = int((adjusted_y / valid_height) * GAME_HEIGHT) if valid_height > 0 else 0
            # 4. 越界保护（确保坐标在游戏分辨率范围内）
            vnc_x = max(0, min(vnc_x, GAME_WIDTH - 1))
            vnc_y = max(0, min(vnc_y, GAME_HEIGHT - 1))
        except Exception as e:
            err_msg = f"坐标转换失败: {str(e)}"
            print(f"❌ {err_msg}")
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(err_msg)
            return MouseResponse()
        # 执行VNC鼠标操作（已优化重试逻辑）
        self.vnc_client.send_mouse_action(vnc_x, vnc_y, action)
        print(f"✅[{get_utc8_timestamp()}]鼠标操作完成 | 最终VNC坐标=({vnc_x}, {vnc_y}) | 动作={action}")
        return MouseResponse()

    def Send(self, request: KeyRequest, context):
        """完整按键动作（新增组合键粘连逻辑）"""
        self.request_counter += 1
        key = request.key
        key_name = self.keys_map.get(key, f"未知按键({key})")
        # 1. 基础按下时长（秒）
        original_down_sec = request.down_ms / 1000.0
        # 2. 获取高精度随机延迟（-10ms ~ +10ms）
        random_delay = self.vnc_client._get_random_delay()
        # 3. 计算随机后的按下时长（确保不小于0.001秒，避免负数）
        randomized_down_sec = max(0.001, original_down_sec + random_delay)
        # 4. 转换回ms并保留15位小数（用于打印展示）
        randomized_down_ms = round(randomized_down_sec * 1000, 15)

        target_key = self.keys_map.get(key)
        if not target_key:
            err_msg = f"不支持的按键: {key_name}"
            print(f"❌ {err_msg}")
            context.set_code(grpc.StatusCode.INVALID_ARGUMENT)
            context.set_details(err_msg)
            return KeyResponse()

        # 新增：组合键粘连逻辑 - 先按下当前按键，延迟释放前一个按键
        # 1. 先按下当前按键（不释放）
        press_success = self.vnc_client.send_key_press(target_key, hold_time=0.0)
        if not press_success:
            print(f"❌ 组合键按下失败: {key_name}")
            return KeyResponse()
        self.key_state_cache[key] = KeyState.Pressed

        # 2. 定义延迟释放函数（使用随机后的按下时长）
        def delay_release(release_key: str, release_key_enum: Key, hold_duration: float):
            time.sleep(hold_duration)
            release_success = self.vnc_client.send_key_release(release_key)
            if release_success:
                self.key_state_cache[release_key_enum] = KeyState.Released

        # 3. 提交延迟释放任务到线程池（替代手动创建线程）
        future = self.release_executor.submit(
            delay_release, target_key, key, randomized_down_sec
        )
        self.release_futures.append(future)  # 跟踪任务，用于后续清理
        # 记录当前键位更新时间（用于缓存清理）
        self.key_state_update_time[key] = time.time()

        return KeyResponse()

    def clean_expired_futures(self):
        """清理已完成的释放任务future，避免内存泄漏"""
        self.release_futures = [f for f in self.release_futures if not f.done()]

    def _clean_loop(self):
        """定时清理过期future和无效键位缓存，每30秒执行一次"""
        while True:
            self.clean_expired_futures()
            # 可选：清理超过1小时未更新的键位状态（按需调整）
            current_time = time.time()
            for key in list(self.key_state_cache.keys()):
                if current_time - self.key_state_update_time.get(key, 0) > 3600:
                    self.key_state_cache[key] = KeyState.Released
            time.sleep(30)  # 每30秒清理一次

    def SendUp(self, request: KeyUpRequest, context):
        """仅释放按键（基于keyPress的释放逻辑）"""
        self.request_counter += 1
        key = request.key
        key_name = self.keys_map.get(key, f"未知按键({key})")
        
        target_key = self.keys_map.get(key)
        if not target_key:
            err_msg = f"不支持的按键: {key_name}"
            print(f"❌ {err_msg}")
            context.set_code(grpc.StatusCode.INVALID_ARGUMENT)
            context.set_details(err_msg)
            return KeyUpResponse()
        # 使用专门的释放方法（基于keyPress逻辑）
        success = self.vnc_client.send_key_release(target_key)
        if success:
            self.key_state_cache[key] = KeyState.Released
        return KeyUpResponse()

    def SendDown(self, request: KeyDownRequest, context):
        """仅按下按键（keyPress保持时间设为0）"""
        self.request_counter += 1
        key = request.key
        key_name = self.keys_map.get(key, f"未知按键({key})")
        
        target_key = self.keys_map.get(key)
        if not target_key:
            err_msg = f"不支持的按键: {key_name}"
            print(f"❌ {err_msg}")
            context.set_code(grpc.StatusCode.INVALID_ARGUMENT)
            context.set_details(err_msg)
            return KeyDownResponse()
        # 调用keyPress，hold_time设为0表示只按不松
        success = self.vnc_client.send_key_press(target_key, hold_time=0.0)
        if success:
            self.key_state_cache[key] = KeyState.Pressed
        return KeyDownResponse()


if __name__ == "__main__":
    # 1. 初始化VNC客户端
    try:
        vnc_client = VNCInputClient(VNC_HOST, VNC_PORT, VNC_PASSWORD)
    except ConnectionError as e:
        print(f"❌ VNC初始化失败: {e}")
        exit(1)
    # 2. 按键映射（保持不变）
    keys_map = {
        # 字母键
        Key.A: "a", Key.B: "b", Key.C: "c", Key.D: "d", Key.E: "e",
        Key.F: "f", Key.G: "g", Key.H: "h", Key.I: "i", Key.J: "j",
        Key.K: "k", Key.L: "l", Key.M: "m", Key.N: "n", Key.O: "o",
        Key.P: "p", Key.Q: "q", Key.R: "r", Key.S: "s", Key.T: "t",
        Key.U: "u", Key.V: "v", Key.W: "w", Key.X: "x", Key.Y: "y", Key.Z: "z",
        # 数字键
        Key.Zero: "0", Key.One: "1", Key.Two: "2", Key.Three: "3", Key.Four: "4",
        Key.Five: "5", Key.Six: "6", Key.Seven: "7", Key.Eight: "8", Key.Nine: "9",
        # 功能键
        Key.F1: "F1", Key.F2: "F2", Key.F3: "F3", Key.F4: "F4", Key.F5: "F5",
        Key.F6: "F6", Key.F7: "F7", Key.F8: "F8", Key.F9: "F9", Key.F10: "F10",
        Key.F11: "F11", Key.F12: "F12",
        # 方向键/控制键
        Key.Up: "UP", Key.Down: "DOWN", Key.Left: "LEFT", Key.Right: "RIGHT",
        Key.Home: "HOME", Key.End: "END", Key.PageUp: "PGUP", Key.PageDown: "PGDN",
        Key.Insert: "INSERT", Key.Delete: "DEL", Key.Esc: "ESC", Key.Enter: "ENTER",
        Key.Space: "SPACE",Key.Backspace: "Backspace",
        # 修饰键
        Key.Ctrl: "ctrl", Key.Shift: "SHIFT", Key.Alt: "ALT",
        # 特殊字符
        Key.Tilde: "`", Key.Quote: "'", Key.Semicolon: ";",
        Key.Comma: ",", Key.Period: ".", Key.Slash: "/",
        Key.Minus: "-", Key.Equal: "=",
        Key.LeftBracket: "[", Key.RightBracket: "]", Key.Backslash: "\\",
    }
    # 3. 初始化KeyInput服务
    key_input_servicer = KeyInput(keys_map, vnc_client)
    # 4. 启动gRPC服务器
    server = grpc.server(ThreadPoolExecutor(max_workers=10))
    add_KeyInputServicer_to_server(key_input_servicer, server)
    server.add_insecure_port('[::]:50051')  # 可根据需求修改端口
    server.start()
    print(f"✅[{get_utc8_timestamp()}]gRPC服务器启动成功，端口: 50051")
    # 5. 等待退出，优雅关闭线程池
    try:
        while True:
            time.sleep(3600)  # 保持服务器运行
    except KeyboardInterrupt:
        print(f"\n⚠️[{get_utc8_timestamp()}]收到退出信号，正在关闭服务器...")
        # 关闭延迟释放线程池（等待未完成任务）
        key_input_servicer.release_executor.shutdown(wait=True, timeout=10)
        # 关闭VNC连接
        vnc_client.close()
        # 关闭gRPC服务器
        server.stop(5)
        print(f"✅[{get_utc8_timestamp()}]服务器已优雅退出")
