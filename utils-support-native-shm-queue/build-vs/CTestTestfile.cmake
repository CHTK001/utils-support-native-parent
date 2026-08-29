# CMake generated Testfile for 
# Source directory: D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue
# Build directory: D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/build-vs
# 
# This file includes the relevant testing commands required for 
# testing this directory and lists subdirectories to be tested as well.
if(CTEST_CONFIGURATION_TYPE MATCHES "^([Dd][Ee][Bb][Uu][Gg])$")
  add_test(shmqueue_basic "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/build-vs/Debug/shmqueue_test.exe")
  set_tests_properties(shmqueue_basic PROPERTIES  _BACKTRACE_TRIPLES "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;94;add_test;D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;0;")
elseif(CTEST_CONFIGURATION_TYPE MATCHES "^([Rr][Ee][Ll][Ee][Aa][Ss][Ee])$")
  add_test(shmqueue_basic "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/build-vs/Release/shmqueue_test.exe")
  set_tests_properties(shmqueue_basic PROPERTIES  _BACKTRACE_TRIPLES "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;94;add_test;D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;0;")
elseif(CTEST_CONFIGURATION_TYPE MATCHES "^([Mm][Ii][Nn][Ss][Ii][Zz][Ee][Rr][Ee][Ll])$")
  add_test(shmqueue_basic "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/build-vs/MinSizeRel/shmqueue_test.exe")
  set_tests_properties(shmqueue_basic PROPERTIES  _BACKTRACE_TRIPLES "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;94;add_test;D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;0;")
elseif(CTEST_CONFIGURATION_TYPE MATCHES "^([Rr][Ee][Ll][Ww][Ii][Tt][Hh][Dd][Ee][Bb][Ii][Nn][Ff][Oo])$")
  add_test(shmqueue_basic "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/build-vs/RelWithDebInfo/shmqueue_test.exe")
  set_tests_properties(shmqueue_basic PROPERTIES  _BACKTRACE_TRIPLES "D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;94;add_test;D:/ch/project/utils-support-native-parent/utils-support-native-shm-queue/CMakeLists.txt;0;")
else()
  add_test(shmqueue_basic NOT_AVAILABLE)
endif()
