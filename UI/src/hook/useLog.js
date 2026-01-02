import { insert, select, selectCustom, deleteData } from '../utils/sqlite'

export function useLog() {
    /**
     * 写入日志
     * @param {string} content 日志内容
     * @param {'INFO' | 'WARN' | 'ERROR'} level 日志级别
     */
    const addLog = (content, level = 'INFO') => {
        insert('logs', ['level', 'content'], [level, content]).catch((error)=>{
            console.log('日志写入失败: ', error);
        });
    };

    /**
     * 分页获取日志
     * @param {number} page 页码 (从1开始)
     * @param {number} pageSize 每页条数
     * @param {string} level 过滤级别 (可选)
     */
    const fetchLogs = (page = 1, pageSize = 10, level = null) => {
        return new Promise((resolve, reject) => {
            var total = 0;
            const offset = (page - 1) * pageSize;
            // 构建查询条件
            let whereClause = '';
            let whereArgs = [];
            if (level) {
                whereClause = 'level = ?';
                whereArgs = [level];
            }

            // 查询总数
            const countSql = `SELECT COUNT(*) as total FROM logs ${whereClause ? 'WHERE ' + whereClause : ''}`;

            selectCustom(countSql, whereArgs).then(result => {
                if(result.rows.length > 0){
                    total = result.rows[0].total;
                }
                // 分页查询数据
                const querySql = `
                    SELECT * FROM logs 
                    ${whereClause ? 'WHERE ' + whereClause : ''} 
                    ORDER BY createTime DESC 
                    LIMIT ? OFFSET ?
                `;
                return selectCustom(querySql, [...whereArgs, pageSize, offset]);
            }).then((result)=>{
                resolve({total, data: result.rows});
            }).catch((error)=>{
                console.error('查询日志失败:', error);
                reject(error);
            });
        });
    };

    /**
     * 清空日志
     */
    const clearLogs = async () => {
        deleteData('logs', '', []).catch((error)=>{
            console.error('清除日志失败:', error);
        })
    };

    return {
        addLog,
        fetchLogs,
        clearLogs
    };
}