/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20231010 	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

public class BaseData {
    private String errorCode;
    private String errorInfo;
    private String subErrorCode;
    private String sign;

    public void setErrorCode(String errorCode) {
        this.errorCode = errorCode;
    }
    public String getErrorCode() {
        return errorCode;
    }

    public void setErrorInfo(String errorInfo) {
        this.errorInfo = errorInfo;
    }
    public String getErrorInfo() {
        return errorInfo;
    }

    public String getSubErrorCode() {
        return subErrorCode;
    }
    public void setSubErrorCode(String subErrorCode) {
        this.subErrorCode = subErrorCode;
    }

    public void setSign(String sign) {
        this.sign = sign;
    }
    public String getSign() {
        return sign;
    }

    public BaseResult getBaseResult(){
        return null;
    }

}
