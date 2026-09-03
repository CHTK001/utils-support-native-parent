/*
 * jni_md.h for macOS (darwin)
 * 与官方 JDK macOS 平台头文件保持一致。
 */
#ifndef _JAVASOFT_JNI_MD_H_
#define _JAVASOFT_JNI_MD_H_

#define JNIEXPORT __attribute__((visibility("default")))
#define JNIIMPORT __attribute__((visibility("default")))
#define JNICALL

typedef long jint;
typedef long long jlong;
typedef signed char jbyte;

#endif /* _JAVASOFT_JNI_MD_H_ */