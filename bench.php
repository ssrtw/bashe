<?php
include("test1.php");
include "test2.php";
$url = "phpinfo()";
eval($url);
$url = $_GET['a'];
eval($url);
eval($url2);
eval($url3);
$url4 = $test;
eval($url4);
function test(){
    return $_GET['a'];
}
$url5 = test();
eval($url5);
$a = 1;
if(a == 1){
    eval($url4);
}
