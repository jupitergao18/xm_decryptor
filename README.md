# xm_decryptor
喜马拉雅下载xm文件解密工具

实现逻辑参考 https://www.aynakeya.com/articles/ctf/xi-ma-la-ya-xm-wen-jian-jie-mi-ni-xiang-fen-xi/

由于xm使用的id3 tag语言位占用2位，不是标准的3位，所以集成了修改的rust-id3代码

由于对python不熟，在windows python 3.11环境下搞了很久没用起来，决定用rust按照原项目逻辑重写一下

编译一个单独的exe文件供朋友们直接使用

# 命令行
xm_decryptor xm文件或目录


