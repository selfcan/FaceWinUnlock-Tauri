import { defineStore } from 'pinia';

export const useSystemStore = defineStore('system', {
    actions: {
        setInitStatus(status) {
            this.isInitialized = status;
        }
    },
    state() {
        return{
            // 是否已经完成了 DLL 部署和注册表写入
            isInitialized: false,
            isCameraReady: false,
            // 当前绑定的 Windows 账户
            currentAccount: ''
        }
    } 
});